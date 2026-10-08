import assert from 'node:assert/strict';
import { mkdtemp, readFile, stat, utimes, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import http from 'node:http';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { PassThrough, Writable } from 'node:stream';
import test from 'node:test';
import { factsFor, serveFixture, taskFor, TOOLS } from './fixture-mcp.mjs';
import { buildPlan, evaluateResult, parseEnv, prepareFixture, pythonUnitTestCommand, redact, sanitizedEvent, selectorOutcomes } from './live-ab.mjs';
import { nestedToolMeasurements, requestToolInventory, safeUsage, sseUsageCollector, startObserver, toolInventory, usageSummary, validateDestination } from './observer.mjs';
import { currentTurnInputs, startMockApis } from './offline.mjs';
import { codingTask, prepareCodingProject, verifyCodingProject } from './coding-fixture.mjs';

test('coding gate rejects the original bug, checks a real fix, and rejects changed tests', async () => {
  const project = await mkdtemp(path.join(tmpdir(), 'overmind-coding-gate-'));
  const code = 'DISP-ABCDEF123456';
  await prepareCodingProject(project);
  assert.equal((await verifyCodingProject(project, code)).passed, false);
  await writeFile(path.join(project, 'dispatch.py'), 'import re\ndef format_dispatch(code):\n    if not isinstance(code, str) or re.fullmatch(r"DISP-[0-9A-F]{12}", code) is None:\n        raise ValueError("Invalid dispatch code")\n    return "dispatch:" + code\n');
  assert.equal((await verifyCodingProject(project, code)).passed, true);
  // Leave a timestamp/size-valid bytecode cache, then corrupt the same source.
  assert.equal(spawnSync('python3', ['-m', 'py_compile', 'dispatch.py'], { cwd: project }).status, 0);
  const sourcePath = path.join(project, 'dispatch.py');
  const previous = await stat(sourcePath);
  const source = await readFile(sourcePath, 'utf8');
  await writeFile(sourcePath, source.replace('"dispatch:" + code', '"dispatch!" + code'));
  await utimes(sourcePath, previous.atime, previous.mtime);
  assert.equal((await verifyCodingProject(project, code)).passed, false);
  await writeFile(path.join(project, 'test_dispatch.py'), '# removed the tests\n');
  assert.equal((await verifyCodingProject(project, code)).test_fixture_unchanged, false);
  assert.equal(codingTask('nonce-ticket').prompt.includes(code), false);
  assert.equal(buildPlan({ tasks: ['coding'] }).length, 4);
});

test('resumed mock turns require a fresh lookup rather than replaying a previous tool result', () => {
  const previous = { type: 'function_call_output', call_id: 'offline-lookup-old', output: 'old result' };
  const user = { type: 'message', role: 'user', content: 'new request' };
  const latest = { type: 'custom_tool_call_output', call_id: 'offline-lookup-new', output: 'new result' };
  assert.deepEqual(currentTurnInputs([previous, user]), [user]);
  assert.deepEqual(currentTurnInputs([previous, user, latest]), [user, latest]);
  assert.deepEqual(currentTurnInputs([latest]), [latest]);
});

test('coding evidence recognizes a successful unittest command without retaining shell text', () => {
  for (const command of ['python3 -m unittest -q', '/bin/bash -lc \'python3 -m unittest -q\'', '/usr/bin/bash -lc "python3 -m unittest -q"']) {
    assert.equal(pythonUnitTestCommand(command), true);
  }
  for (const command of ['cat dispatch.py', 'echo python3 -m unittest -q', 'false && python3 -m unittest -q || true', 'python3 -m unittest -q; true']) {
    assert.equal(pythonUnitTestCommand(command), false);
  }
  const event = sanitizedEvent({ type: 'item.completed', item: { type: 'command_execution', command: 'python3 -m unittest -q', exit_code: 0 } }, []);
  assert.equal(event.item.python_unit_tests, true);
  assert.equal(event.item.exit_code, 0);
  assert.equal(event.item.command, undefined);
});

test('credential parsing treats shell syntax as data and never needs OAuth copying', () => {
  const text = '# OPENAI_API_KEY=no\nexport OPENAI_API_KEY="first"\nOTHER=ignored\nOPENAI_API_KEY=second # comment\n';
  assert.equal(parseEnv(text, 'OPENAI_API_KEY'), 'second');
  assert.equal(parseEnv('OPENAI_API_KEY=$(touch /tmp/should-not-exist)', 'OPENAI_API_KEY'), '$(touch /tmp/should-not-exist)');
  assert.equal(parseEnv('OPENAI_API_KEY="unterminated', 'OPENAI_API_KEY'), null);
  assert.equal(parseEnv('CURSOR_API_KEY=value', 'OPENAI_API_KEY'), null);
});

test('nonce-backed tasks require real matching MCP calls as well as a correct answer', () => {
  const ticket = 'ticket-one';
  const seed = 'private-fixture-seed';
  const task = taskFor('single', ticket);
  const expected = factsFor(seed, ticket).dispatch_code;
  assert.equal(task.prompt.includes(expected), false);
  assert.notEqual(expected, factsFor(seed, 'another-ticket').dispatch_code);
  const events = [{ type: 'turn.completed', usage: {} }, { type: 'item.completed', item: { type: 'agent_message', text: expected } }];
  const common = { task, ticket, seed, events, selectors: [], arm: 'plain', exitCode: 0, timedOut: false, requestLimit: false };
  assert.equal(evaluateResult({ ...common, audit: [] }).success, false);
  const audit = [{ type: 'tools.call', name: 'shipping_dispatch', ok: true, valid_ticket: true }];
  assert.equal(evaluateResult({ ...common, audit }).success, true);
  assert.equal(evaluateResult({ ...common, audit, arm: 'decisions' }).success, false);
  assert.equal(evaluateResult({ ...common, audit: [...audit, { type: 'tools.call', name: 'invoice_balance', ok: true, valid_ticket: true }] }).success, false);
});

test('MCP fixture exposes all read-only tools, rejects wrong tickets, and emits progress', async () => {
  const input = new PassThrough();
  const messages = [];
  const output = new Writable({ write(chunk, _encoding, callback) { messages.push(JSON.parse(chunk)); callback(); } });
  const serving = serveFixture({ input, output, env: { OVERMIND_FIXTURE_SEED: 'test-seed', OVERMIND_FIXTURE_TICKET: 'test-ticket' } });
  for (const message of [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18' } },
    { jsonrpc: '2.0', id: 2, method: 'tools/list' },
    { jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'shipping_dispatch', arguments: { ticket: 'wrong' } } },
    { jsonrpc: '2.0', id: 4, method: 'tools/call', params: { name: 'progress_delayed_report', arguments: { ticket: 'test-ticket' }, _meta: { progressToken: 'progress-one' } } },
  ]) input.write(`${JSON.stringify(message)}\n`);
  input.end();
  await serving;
  assert.equal(messages.find((message) => message.id === 2).result.tools.length, 24);
  assert.ok(TOOLS.every((tool) => tool.annotations.readOnlyHint && !tool.annotations.destructiveHint));
  assert.equal(messages.find((message) => message.id === 3).result.isError, true);
  const progress = messages.filter((message) => message.method === 'notifications/progress');
  assert.deepEqual(progress.map((message) => message.params.progress), [1, 2, 3, 4]);
  assert.ok(progress.every((message) => message.params.progressToken === 'progress-one'));
  assert.equal(messages.find((message) => message.id === 4).result.structuredContent.report_code, factsFor('test-seed', 'test-ticket').report_code);
});

test('observation accepts only official OpenAI or an explicitly owned loopback destination', () => {
  assert.equal(validateDestination('https://api.openai.com/v1/').hostname, 'api.openai.com');
  assert.equal(validateDestination('http://127.0.0.1:4321/v1/', { loopbackPort: 4321 }).port, '4321');
  for (const url of ['https://example.com/v1/', 'http://api.openai.com/v1/', 'https://api.openai.com.evil/v1/', 'https://secret@api.openai.com/v1/', 'https://api.openai.com/v1/?key=secret', 'http://127.0.0.1:4321/v1/']) {
    assert.throws(() => validateDestination(url));
  }
});

test('SSE measurement collects usage and tool counts across fragmented frames without retaining content', () => {
  const record = { tool_search_calls: 0, function_calls: 0, has_agent_message: false };
  const collect = sseUsageCollector(record);
  const frames = [
    { type: 'response.output_item.done', item: { type: 'tool_search_call', arguments: { query: 'private-query-should-not-persist' } } },
    { type: 'response.output_item.done', item: { type: 'function_call', arguments: 'private-tool-arguments' } },
    { type: 'response.output_item.done', item: { type: 'custom_tool_call', name: 'exec', input: 'text(ALL_TOOLS.filter(({name}) => name.includes("private-query")));' } },
    { type: 'response.completed', response: { usage: { input_tokens: 100, output_tokens: 20, input_tokens_details: { cached_tokens: 80 } }, output: [{ type: 'message', role: 'assistant', content: [{ text: 'private-response-body' }] }] } },
  ].map((event) => `data: ${JSON.stringify(event)}\n\n`).join('');
  for (let index = 0; index < frames.length; index += 7) collect.write(Buffer.from(frames.slice(index, index + 7)));
  collect.end();
  assert.equal(record.tool_search_calls, 1);
  assert.equal(record.function_calls, 2);
  assert.equal(record.catalog_discovery_attempts, 1);
  assert.equal(record.has_agent_message, true);
  assert.equal(record.usage.cached_input_tokens, 80);
  assert.equal(JSON.stringify(record).includes('private-'), false);
  assert.deepEqual(safeUsage({ input_tokens: 'wrong', output_tokens: -1, authorization: 'secret' }), {});
});

test('tool inventory distinguishes direct preselection from deferred schemas', () => {
  const inventory = toolInventory([{ type: 'tool_search' }, { type: 'namespace', name: 'mcp__fixture', tools: [
    { type: 'function', name: 'shipping_dispatch', defer_loading: false }, { type: 'function', name: 'invoice_balance', defer_loading: true },
  ] }]);
  assert.deepEqual(inventory, { direct_fixture_tools: ['shipping_dispatch'], deferred_fixture_tools: ['invoice_balance'],
    code_mode_fixture_tools: [], code_mode_exec_available: false, tool_search_available: true });
});

test('CodeModeOnly inventory requires full exec declarations and deduplicates nested search history', () => {
  const inventory = toolInventory([{ type: 'custom', name: 'exec', description: 'Some deferred metadata mentions mcp__fixture__invoice_balance.\n### `mcp__fixture__shipping_dispatch`\nFull declaration.\n### `tool_search`\nSearch declaration.' }]);
  assert.deepEqual(inventory.code_mode_fixture_tools, ['shipping_dispatch']);
  assert.equal(inventory.code_mode_exec_available, true);
  assert.equal(inventory.direct_fixture_tools.length, 0);
  assert.equal(inventory.tool_search_available, true);
  const input = [{ call_id: 'harmless-call', internal_chat_message_metadata_passthrough: { cell_id: 'cell-one', executed_tool_calls: [
    { name: 'tool_search', arguments: { query: 'private-query-must-not-persist' } },
    { name: 'mcp__fixture__shipping_dispatch', arguments: { ticket: 'private-input' } },
  ] } }];
  const seen = new Set();
  const measured = nestedToolMeasurements(input, seen);
  assert.deepEqual(measured, { nested_tool_search_calls: 1, executed_fixture_tools: ['shipping_dispatch'] });
  assert.deepEqual(nestedToolMeasurements(input, seen), { nested_tool_search_calls: 0, executed_fixture_tools: [] });
  assert.equal(JSON.stringify(measured).includes('private-'), false);
});

test('Responses Lite additional_tools exposes native exec schemas without scanning message text', () => {
  const inventory = requestToolInventory({ input: [
    { type: 'message', role: 'developer', content: '### `mcp__fixture__invoice_balance`' },
    { type: 'additional_tools', role: 'developer', tools: [{ type: 'namespace', name: 'functions', tools: [
      { type: 'custom', name: 'exec', description: '### `mcp__fixture__shipping_dispatch`\nFull declaration.' },
    ] }] },
  ] });
  assert.equal(inventory.code_mode_exec_available, true);
  assert.deepEqual(inventory.code_mode_fixture_tools, ['shipping_dispatch']);
  assert.deepEqual(inventory.direct_fixture_tools, []);
  assert.equal(inventory.tool_search_available, false);
});

test('Cursor accounting preserves exact SDK components and excludes estimated intermediate usage', () => {
  const records = [
    { kind: 'responses', completed: true, has_agent_message: false, usage: { input_tokens: 900, output_tokens: 7, usage_status: 'sdk' } },
    { kind: 'responses', completed: true, has_agent_message: true, usage: { input_tokens: 1000, output_tokens: 8, usage_status: 'sdk', model_steps: 2,
      run_input_tokens: 100, run_output_tokens: 20, run_cache_read_input_tokens: 700, run_cache_creation_input_tokens: 60 } },
  ];
  const usage = usageSummary(records, 'cursor');
  assert.equal(usage.sdk_input_tokens, 100);
  assert.equal(usage.sdk_cache_read_input_tokens, 700);
  assert.equal(usage.input_tokens, 860);
  assert.equal(usage.uncached_input_tokens, 160);
  assert.equal(usage.output_tokens, 20);
  assert.equal(usage.complete, true);
  records[0].has_agent_message = true;
  assert.equal(usageSummary(records, 'cursor').output_tokens, 20);
  assert.equal(usageSummary(records, 'cursor').complete, true);
  delete records[1].usage.run_cache_creation_input_tokens;
  assert.equal(usageSummary(records, 'cursor').uncached_input_tokens, null);
  assert.equal(usageSummary(records, 'cursor').input_tokens, null);
  assert.equal(usageSummary(records, 'cursor').complete, false);
  records[1].usage.run_cache_creation_input_tokens = 60;
  records[0].completed = false;
  assert.equal(usageSummary(records, 'cursor').complete, false);
  records[0].completed = true;
  records[1].usage.model_steps = 1;
  assert.equal(usageSummary(records, 'cursor').complete, false);
});

test('OpenAI accounting keeps missing usage unknown and reports uncached input correctly', () => {
  const records = [{ kind: 'responses', completed: true, usage: { input_tokens: 100, cached_input_tokens: 80, output_tokens: 10 } }];
  assert.equal(usageSummary(records, 'openai').uncached_input_tokens, 20);
  records.push({ kind: 'responses', completed: true, usage: null });
  assert.equal(usageSummary(records, 'openai').complete, false);
  assert.equal(usageSummary([], 'openai').complete, false);
  assert.equal(usageSummary([], 'cursor').complete, false);
});

test('Cursor lineage accounting uses the latest cumulative totals and verifies full call coverage', () => {
  const lineage = { input_tokens: 120, output_tokens: 30, cache_read_input_tokens: 700,
    cache_creation_input_tokens: 60, reasoning_tokens: 5, model_steps: 3, covered_model_steps: 3,
    run_count: 2, missing_runs: 0, complete: true };
  const records = [
    { kind: 'responses', completed: true, usage: { input_tokens: 900, output_tokens: 7, usage_status: 'sdk' } },
    { kind: 'responses', completed: true, has_agent_message: true, usage: { usage_status: 'sdk', lineage_usage: { ...lineage, input_tokens: 80 } } },
    { kind: 'responses', completed: true, has_agent_message: true, usage: { usage_status: 'sdk', model_steps: 2,
      run_input_tokens: 100, run_output_tokens: 20, run_cache_read_input_tokens: 600,
      run_cache_creation_input_tokens: 50, lineage_usage: lineage } },
  ];
  const usage = usageSummary(records, 'cursor');
  assert.equal(usage.source, 'cursor_sdk_lineage_totals');
  assert.equal(usage.complete, true);
  assert.equal(usage.sdk_input_tokens, 120);
  assert.equal(usage.input_tokens, 880);
  assert.equal(usage.uncached_input_tokens, 180);
  assert.equal(usage.output_tokens, 30);
  lineage.complete = false;
  assert.equal(usageSummary(records, 'cursor').complete, false);
  lineage.complete = true;
  lineage.covered_model_steps = 2;
  assert.equal(usageSummary(records, 'cursor').complete, false);
  lineage.covered_model_steps = 3;
  lineage.missing_runs = 1;
  assert.equal(usageSummary(records, 'cursor').complete, false);
  lineage.missing_runs = 0;
  delete lineage.cache_creation_input_tokens;
  assert.equal(usageSummary(records, 'cursor').input_tokens, null);
  assert.equal(usageSummary(records, 'cursor').complete, false);
});

test('lineage measurement retains only nonnegative integer counters and a strict boolean', () => {
  const measured = safeUsage({ lineage_usage: { input_tokens: 10, output_tokens: -1,
    model_steps: 1.5, run_count: '2', complete: 'true', secret: 'do-not-retain' } });
  assert.deepEqual(measured, { lineage_usage: { input_tokens: 10 } });
  assert.equal(JSON.stringify(measured).includes('do-not-retain'), false);
});

test('selector trace parsing supports native formatting and removes unrelated trace fields', () => {
  const outcome = { mode: 'decisions', status: 'selected', candidate_count: 24, request_count: 1, selected_names: ['mcp__fixture.shipping_dispatch'], cache_hit: false, prompt: 'must-not-retain', selector_usage: { input_tokens: 10, output_tokens: 2 } };
  const raw = `INFO overmind::tool_selector: selector_outcome=${JSON.stringify(outcome)}`;
  const quoted = `INFO overmind::tool_selector: selector_outcome=${JSON.stringify(JSON.stringify(outcome))}`;
  assert.equal(selectorOutcomes(raw)[0].status, 'selected');
  assert.deepEqual(selectorOutcomes(raw), selectorOutcomes(quoted));
  assert.equal(JSON.stringify(selectorOutcomes(raw)).includes('must-not-retain'), false);
  assert.equal(selectorOutcomes(`${raw}\n${raw}`).length, 2);
  assert.equal(selectorOutcomes(`selector_outcome=${JSON.stringify({ ...outcome, selector_usage: { input_tokens: 10, output_tokens: null, total_tokens: null } })}`)[0].selector_usage.output_tokens, null);
  assert.deepEqual(selectorOutcomes('selector_outcome={partial'), []);
});

test('sanitized CLI events recursively redact credentials and omit reasoning', () => {
  const secret = 'fixture-api-secret-12345';
  const event = sanitizedEvent({ type: 'item.completed', item: { type: 'mcp_tool_call', arguments: { nested: [secret] }, result: { content: [{ type: 'text', text: `Bearer ${secret}` }] } } }, [secret]);
  assert.equal(JSON.stringify(event).includes(secret), false);
  assert.equal(redact(`prefix ${secret}`, [secret]), 'prefix [REDACTED]');
  assert.equal(sanitizedEvent({ type: 'item.completed', item: { type: 'reasoning', text: 'private reasoning' } }, []).item.text, undefined);
});

test('paired plan caps jobs, shares a fresh challenge within each pair, and balances arm order', () => {
  const plan = buildPlan({ reps: 2 });
  assert.equal(plan.length, 24);
  assert.equal(plan[0].arm, 'plain');
  assert.equal(plan[12].arm, 'decisions');
  for (const pair of new Set(plan.map((job) => job.pair))) {
    const jobs = plan.filter((job) => job.pair === pair);
    assert.equal(jobs.length, 2);
    assert.equal(jobs[0].ticket, jobs[1].ticket);
    assert.equal(jobs[0].seed, jobs[1].seed);
  }
  assert.equal(new Set(plan.map((job) => job.ticket)).size, 12);
  assert.throws(() => buildPlan({ reps: 3 }));
  assert.throws(() => buildPlan({ providers: ['openai', 'openai'] }));
  assert.throws(() => buildPlan({ tasks: [] }));
});

test('fixture config uses native provider hooks and contains no key material', async () => {
  const jobDir = await mkdtemp(path.join(tmpdir(), 'overmind-config-test-'));
  const fixture = await prepareFixture({ jobDir, provider: 'openai', model: 'gpt-6-luna', arm: 'decisions', ticket: 'test-ticket', auditFile: path.join(jobDir, 'audit.jsonl'), observerUrl: 'http://127.0.0.1:1234/v1' });
  const config = await readFile(path.join(fixture.codexHome, 'config.toml'), 'utf8');
  assert.ok(config.includes('model_provider = "openai"'));
  assert.ok(config.includes('openai_base_url = "http://127.0.0.1:1234/v1"'));
  assert.equal(config.includes('[model_providers.openai]'), false);
  assert.equal(config.includes('code_mode ='), false);
  const mcp = JSON.parse(await readFile(path.join(fixture.codexHome, 'plugins/cache/overmind-benchmark/fixture-tools/local/.mcp.json'), 'utf8'));
  assert.deepEqual(mcp.mcpServers.fixture.env_vars, ['OVERMIND_FIXTURE_SEED']);
  assert.equal(JSON.stringify(mcp).includes('OPENAI_API_KEY'), false);
});

test('owned model mock preserves CodeModeOnly and supports the progress task without real provider calls', async () => {
  const ticket = 'progress-ticket';
  const seed = 'progress-private-seed';
  const mock = await startMockApis({ scenario: 'selected', taskId: 'progress', ticket, seed });
  try {
    assert.equal((await fetch(`${mock.modelTarget}responses`)).status, 426);
    assert.equal(mock.stats().model_requests, 0);
    const request = (body) => fetch(`${mock.modelTarget}responses`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) }).then((response) => response.text());
    const first = await request({ input: [{ type: 'additional_tools', role: 'developer', tools: [{ type: 'namespace', name: 'functions', tools: [
      { type: 'custom', name: 'exec', description: '### `mcp__fixture__progress_delayed_report`\nFull declaration' },
    ] }] }] });
    assert.ok(first.includes('custom_tool_call'));
    assert.ok(first.includes('tools.mcp__fixture__progress_delayed_report'));
    const second = await request({ tools: [], input: [{ type: 'custom_tool_call_output', call_id: 'offline-lookup-1', output: 'synthetic tool output' }] });
    assert.ok(second.includes(factsFor(seed, ticket).report_code));
    assert.equal(mock.stats().model_inventories[0].code_mode_exec_available, true);
    const decisions = await fetch(mock.decisionsEndpoint, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({
      input: JSON.stringify({ tools: [{ id: 't0', name: { namespace: 'mcp__fixture', name: 'progress_delayed_report' } }] }), questions: [{ type: 'predicate', name: 't0' }],
    }) }).then((response) => response.json());
    assert.equal(decisions.answers[0].probability, 0.99);
  } finally { await mock.close(); }
});

test('owned mock keeps its default cap and permits a bounded multi-turn UI budget', async () => {
  await assert.rejects(startMockApis({ maxModelRequests: 25 }), /request limit/);
  for (const configuredLimit of [undefined, 9]) {
    const limit = configuredLimit ?? 8;
    const mock = await startMockApis({ scenario: 'plain', ticket: 'bounded-ticket', seed: 'bounded-seed', maxModelRequests: configuredLimit });
    try {
      for (let index = 1; index <= limit + 1; index += 1) {
        const response = await fetch(`${mock.modelTarget}responses`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"input":[]}' });
        assert.equal(response.status, index <= limit ? 200 : 429);
        await response.text();
      }
    } finally { await mock.close(); }
  }
});

test('native CodeModeOnly lazy mock discovers ALL_TOOLS before executing an MCP tool', async () => {
  const mock = await startMockApis({ scenario: 'plain', ticket: 'lazy-ticket', seed: 'lazy-seed' });
  try {
    const request = (input) => fetch(`${mock.modelTarget}responses`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({
      tools: [{ type: 'custom', name: 'exec', description: 'Runtime guidance without preloaded fixture declarations.' }], input,
    }) }).then((response) => response.text());
    const discovery = await request([]);
    assert.ok(discovery.includes('custom_tool_call'));
    assert.ok(discovery.includes('ALL_TOOLS.filter'));
    assert.equal(discovery.includes('tool_search_call'), false);
    const execution = await request([{ type: 'custom_tool_call_output', call_id: 'offline-catalog-1', output: 'Fixture schema returned by actual executor in full CLI tests.' }]);
    assert.ok(execution.includes('tools.mcp__fixture__shipping_dispatch'));
    assert.equal(execution.includes('ALL_TOOLS.filter'), false);
  } finally { await mock.close(); }
});

test('loopback observer enforces its request cap and never records credentials or request bodies', async () => {
  let requests = 0;
  let reachedLimit = false;
  const upstream = http.createServer((request, response) => {
    requests += 1;
    request.resume();
    response.writeHead(200, { 'content-type': 'text/event-stream' });
    response.end('data: {"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":1,"input_tokens_details":{"cached_tokens":20}}}}\n\n');
  });
  await new Promise((resolve) => upstream.listen(0, '127.0.0.1', resolve));
  const port = upstream.address().port;
  const observer = await startObserver({ target: `http://127.0.0.1:${port}/v1/`, loopbackPort: port, maxRequests: 1, onLimit: () => { reachedLimit = true; } });
  try {
    const secret = 'fixture-secret-123456';
    const fallback = await fetch(`${observer.url}/responses`);
    assert.equal(fallback.status, 426);
    assert.equal(requests, 0);
    assert.equal(observer.records[0].kind, 'websocket_fallback');
    const first = await fetch(`${observer.url}/responses`, { method: 'POST', headers: { authorization: `Bearer ${secret}`, 'content-type': 'application/json' }, body: JSON.stringify({ private_input: 'must-not-retain', tools: [{ type: 'tool_search' }] }) });
    await first.text();
    assert.equal(first.status, 200);
    const second = await fetch(`${observer.url}/responses`, { method: 'POST', body: '{}' });
    assert.equal(second.status, 429);
    assert.equal(requests, 1);
    assert.equal(reachedLimit, true);
    const recorded = JSON.stringify(observer.records);
    assert.equal(recorded.includes(secret), false);
    assert.equal(recorded.includes('must-not-retain'), false);
  } finally {
    await observer.close();
    upstream.closeAllConnections();
    await new Promise((resolve) => upstream.close(resolve));
  }
});
