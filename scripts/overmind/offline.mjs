#!/usr/bin/env node
// Exercise the real built CLI + MCP registry using only owned loopback endpoints.
import { randomBytes } from 'node:crypto';
import http from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
import { factsFor, taskFor } from './fixture-mcp.mjs';
import { repoRoot, runJob } from './live-ab.mjs';
import { requestToolInventory } from './observer.mjs';

async function listen(server) {
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  return { port, url: `http://127.0.0.1:${port}` };
}

async function close(server) {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}

function eventStream(items, requestIndex) {
  const id = `offline-response-${requestIndex}`;
  const events = [
    { type: 'response.created', response: { id } },
    ...items.map((item) => ({ type: 'response.output_item.done', item })),
    { type: 'response.completed', response: { id, status: 'completed', output: items,
      usage: { input_tokens: 100, input_tokens_details: { cached_tokens: 20 }, output_tokens: 10, output_tokens_details: { reasoning_tokens: 0 }, total_tokens: 110 } } },
  ];
  return events.map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join('');
}

export async function startMockApis({ scenario, ticket, seed, taskId = 'single', maxModelRequests = 8 }) {
  if (!Number.isInteger(maxModelRequests) || maxModelRequests < 1 || maxModelRequests > 24) {
    throw new Error('Owned mock requires a model request limit between 1 and 24.');
  }
  let modelRequests = 0;
  let selectorRequests = 0;
  let websocketAttempts = 0;
  const selectorInventories = [];
  const modelInventories = [];
  const timers = new Set();
  const task = taskFor(taskId, ticket);
  const catalog = await readFile(path.join(repoRoot, 'codex-rs/models-manager/models.json'));
  const model = http.createServer(async (request, response) => {
    if (request.method === 'GET' && request.url?.split('?')[0] === '/v1/responses') {
      response.writeHead(++websocketAttempts <= maxModelRequests ? 426 : 429, { connection: 'close' }); response.end(); return;
    }
    if (request.method === 'GET' && request.url?.split('?')[0] === '/v1/models') {
      response.writeHead(200, { 'content-type': 'application/json' }); response.end(catalog); return;
    }
    if (request.method !== 'POST' || request.url !== '/v1/responses') { response.writeHead(404); response.end(); return; }
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks));
    modelRequests += 1;
    if (modelRequests > maxModelRequests) { response.writeHead(429); response.end(); return; }
    const inputs = Array.isArray(body.input) ? body.input : [];
    const called = inputs.some((item) => ['function_call_output', 'custom_tool_call_output'].includes(item.type) && String(item.call_id).startsWith('offline-lookup'));
    const searched = inputs.some((item) => item.type === 'tool_search_output'
      || item.type === 'custom_tool_call_output' && String(item.call_id).startsWith('offline-catalog'));
    const inventory = requestToolInventory(body);
    modelInventories.push(inventory);
    const preloaded = task.requiredTools.every((name) => [...inventory.direct_fixture_tools, ...inventory.code_mode_fixture_tools].includes(name));
    let items;
    if (called) {
      const facts = factsFor(seed, ticket);
      const text = task.fields.length === 1 ? String(facts[task.fields[0]]) : JSON.stringify(Object.fromEntries(task.fields.map((field) => [field, facts[field]])));
      items = [{ type: 'message', role: 'assistant', id: `msg-${modelRequests}`, content: [{ type: 'output_text', text }] }];
    } else if (preloaded || searched) {
      if (inventory.code_mode_exec_available) {
        items = [{ type: 'custom_tool_call', call_id: `offline-lookup-${modelRequests}`, name: 'exec',
          input: task.requiredTools.map((name) => `text(await tools.mcp__fixture__${name}(${JSON.stringify({ ticket })}));`).join('\n') }];
      } else {
        items = task.requiredTools.map((name, index) => ({ type: 'function_call', call_id: `offline-lookup-${modelRequests}-${index}`, namespace: 'mcp__fixture', name, arguments: JSON.stringify({ ticket }) }));
      }
    } else {
      if (inventory.code_mode_exec_available) {
        const names = task.requiredTools.map((name) => `mcp__fixture__${name}`);
        items = [{ type: 'custom_tool_call', call_id: `offline-catalog-${modelRequests}`, name: 'exec',
          input: `text(ALL_TOOLS.filter(({ name }) => ${JSON.stringify(names)}.includes(name)));` }];
      } else {
        items = [{ type: 'tool_search_call', call_id: `offline-search-${modelRequests}`, execution: 'client', arguments: { query: task.requiredTools.join(' ') } }];
      }
    }
    response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
    response.end(eventStream(items, modelRequests));
  });
  const decisions = http.createServer(async (request, response) => {
    if (request.method !== 'POST' || request.url !== '/v1/decisions') { response.writeHead(404); response.end(); return; }
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks));
    selectorRequests += 1;
    let input;
    try { input = JSON.parse(body.input); } catch { input = {}; }
    selectorInventories.push({ count: input.tools?.length ?? 0, names: (input.tools ?? []).map((tool) => tool.name) });
    const reply = () => {
      if (response.destroyed) return;
      if (scenario === 'http_error') {
        response.writeHead(503, { 'content-type': 'application/json' }); response.end(JSON.stringify({ error: { message: 'Synthetic offline service failure' } })); return;
      }
      const answers = (body.questions ?? []).map((question) => {
        const tool = input.tools?.find((candidate) => candidate.id === question.name);
        if (scenario === 'refusal') return { type: 'refusal', name: question.name };
        return { type: 'predicate', name: question.name, probability: scenario === 'empty' ? 0.01 : task.requiredTools.includes(tool?.name?.name) ? 0.99 : 0.01 };
      });
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ answers: scenario === 'invalid' ? [] : answers, usage: { input_tokens: 321, output_tokens: 24, total_tokens: 345 } }));
    };
    if (scenario === 'timeout') {
      const timer = setTimeout(() => { timers.delete(timer); reply(); }, 600);
      timers.add(timer);
    } else reply();
  });
  const modelAddress = await listen(model);
  const decisionsAddress = await listen(decisions);
  return {
    modelTarget: `${modelAddress.url}/v1/`, decisionsEndpoint: `${decisionsAddress.url}/v1/decisions`,
    stats: () => ({ model_requests: modelRequests, selector_requests: selectorRequests, websocket_fallback_attempts: websocketAttempts, selector_inventories: selectorInventories, model_inventories: modelInventories }),
    async close() { for (const timer of timers) clearTimeout(timer); await close(model); await close(decisions); },
  };
}

export async function runOffline({ binary, output, cases = ['plain', 'selected', 'empty', 'http_error', 'timeout', 'invalid', 'refusal'], modes = ['direct', 'code_mode_only'] }) {
  if (cases.some((value) => !['plain', 'selected', 'empty', 'http_error', 'timeout', 'invalid', 'refusal'].includes(value))) throw new Error('Unknown offline scenario.');
  if (modes.some((value) => !['direct', 'code_mode_only'].includes(value)) || modes.length === 0) throw new Error('Offline modes are direct and code_mode_only.');
  await mkdir(output, { recursive: false, mode: 0o700 });
  const results = [];
  for (const mode of modes) for (const scenario of cases) {
    const ticket = `TICKET-${randomBytes(8).toString('hex')}`;
    const seed = randomBytes(24).toString('hex');
    const mock = await startMockApis({ scenario, ticket, seed });
    try {
      const summary = await runJob({ binary, jobDir: path.join(output, `${mode}-${scenario}`), provider: mode === 'direct' ? 'cursor' : 'openai', model: mode === 'direct' ? 'composer-2.5' : 'gpt-6.1-sol', arm: scenario === 'plain' ? 'plain' : 'decisions',
        ticket, seed, credentials: { openai: 'offline-fixture-not-a-real-api-key', cursor: 'offline-fixture-not-a-real-cursor-key' }, taskId: 'single',
        timeoutMs: 30_000, maxRequests: 8, selectorEndpoint: mock.decisionsEndpoint, selectorTimeoutMs: scenario === 'timeout' ? 150 : 2000,
        mockTarget: mock.modelTarget });
      const stats = mock.stats();
      const expectedStatus = scenario === 'plain' ? 'disabled' : scenario === 'selected' ? 'selected' : scenario === 'empty' ? 'empty' : 'fallback';
      const correctOutcome = summary.selector_outcomes.some((outcome) => outcome.status === expectedStatus);
      const searchCorrect = mode === 'code_mode_only' ? summary.tool_search_calls === 0
        && (scenario === 'selected' ? summary.catalog_discovery_attempts === 0 : summary.catalog_discovery_attempts >= 1)
        : summary.catalog_discovery_attempts === 0 && (scenario === 'selected' ? summary.tool_search_calls === 0 : summary.tool_search_calls >= 1);
      const requestCountCorrect = scenario === 'plain' ? stats.selector_requests === 0 : stats.selector_requests === 1;
      const first = stats.model_inventories[0];
      const modeCorrect = first?.code_mode_exec_available === (mode === 'code_mode_only') && (mode !== 'code_mode_only' || first.direct_fixture_tools.length === 0);
      const preloadCorrect = scenario === 'selected'
        ? (mode === 'code_mode_only' ? first?.code_mode_fixture_tools : first?.direct_fixture_tools)?.includes('shipping_dispatch')
        : first?.direct_fixture_tools.length === 0 && first?.code_mode_fixture_tools.length === 0;
      const passed = summary.success && correctOutcome && searchCorrect && requestCountCorrect && modeCorrect && preloadCorrect;
      results.push({ scenario, tool_mode: mode, passed, mode_correct: modeCorrect, preload_correct: preloadCorrect, expected_status: expectedStatus, ...stats, ...summary });
      console.log(JSON.stringify({ scenario, mode, passed, mode_correct: modeCorrect, preload_correct: preloadCorrect, correct_tool_execution: summary.required_tool_calls_proved, final_correct: summary.final_correct,
        selector_status: summary.selector_outcomes.map((outcome) => outcome.status), tool_search_calls: summary.tool_search_calls,
        catalog_discovery_attempts: summary.catalog_discovery_attempts, selector_requests: stats.selector_requests }));
    } finally { await mock.close(); }
  }
  await writeFile(path.join(output, 'offline-results.json'), JSON.stringify({ inference: 'Owned loopback mock endpoints only; no real credentials or provider requests.', results }, null, 2) + '\n', { mode: 0o600 });
  return results;
}

async function main() {
  const args = {};
  for (let index = 2; index < process.argv.length; index += 2) args[process.argv[index].replace(/^--/, '')] = process.argv[index + 1];
  if (!args.binary) throw new Error('Provide --binary with the actual freshly built Overmind executable.');
  const output = path.resolve(args.output ?? path.join(tmpdir(), `overmind-offline-${Date.now()}`));
  const results = await runOffline({ binary: path.resolve(args.binary), output, cases: args.cases?.split(','), modes: args.modes?.split(',') });
  console.log(JSON.stringify({ output, passed: results.filter((result) => result.passed).length, total: results.length }));
  if (results.some((result) => !result.passed)) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch((error) => { console.error(error.message); process.exitCode = 1; });
