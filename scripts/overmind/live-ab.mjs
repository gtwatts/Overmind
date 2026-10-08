#!/usr/bin/env node
// Explicitly scheduled live validation of the built Overmind CLI. No dependencies.
import { createHash, randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { access, chmod, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { homedir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { factsFor, taskFor, TASK_IDS } from './fixture-mcp.mjs';
import { startObserver, usageSummary } from './observer.mjs';
import { codingTask, prepareCodingProject, verifyCodingProject } from './coding-fixture.mjs';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(scriptDir, '../..');
const allowedProviders = ['openai', 'cursor'];
const allowedArms = ['plain', 'decisions'];

export function parseEnv(text, name) {
  let found = null;
  for (const line of text.split(/\r?\n/)) {
    const match = line.trim().match(/^(?:export\s+)?([A-Z][A-Z0-9_]*)\s*=\s*(.*)$/);
    if (!match || match[1] !== name) continue;
    let value = match[2].trim();
    if (value.startsWith('"') || value.startsWith("'")) {
      if (value.at(-1) !== value[0]) continue;
      value = value.slice(1, -1);
    } else value = value.replace(/\s+#.*$/, '').trim();
    if (value) found = value; // Read values as data. Never source/evaluate a file.
  }
  return found;
}

async function optionalFile(file) {
  try { return await readFile(file, 'utf8'); } catch (error) { if (error.code === 'ENOENT') return ''; throw new Error(`Cannot read credential source ${path.basename(file)}.`); }
}

export async function readCredentials(sourceHome, env = process.env) {
  const openai = env.OPENAI_API_KEY?.trim() || parseEnv(await optionalFile(path.join(sourceHome, 'secrets/openai.env')), 'OPENAI_API_KEY');
  const cursor = env.CURSOR_API_KEY?.trim() || parseEnv(await optionalFile(path.join(sourceHome, 'secrets/cursor.env')), 'CURSOR_API_KEY');
  return { openai, cursor };
}

export function redact(value, secrets = []) {
  let text = String(value).replace(/\x1b\[[0-9;]*m/g, '');
  for (const secret of secrets.filter((item) => typeof item === 'string' && item.length > 3)) text = text.split(secret).join('[REDACTED]');
  return text.replace(/\bBearer\s+\S+/gi, 'Bearer [REDACTED]').replace(/\b(?:sk-|key_)[A-Za-z0-9_-]{12,}\b/g, '[REDACTED]');
}

function safeOutcome(value) {
  const result = {};
  for (const field of ['mode', 'status', 'model', 'fallback_reason']) {
    if (typeof value[field] === 'string') result[field] = value[field].slice(0, 160);
    else if (value[field] === null) result[field] = null;
  }
  for (const field of ['candidate_count', 'request_count', 'duration_ms']) if (Number.isFinite(value[field])) result[field] = value[field];
  if (typeof value.cache_hit === 'boolean') result.cache_hit = value.cache_hit;
  if (Array.isArray(value.selected_names)) result.selected_names = value.selected_names.filter((name) => typeof name === 'string' && /^[\w.-]{1,180}$/.test(name));
  if (Array.isArray(value.selected)) result.selected = value.selected.filter((tool) => typeof tool?.name === 'string').map((tool) => ({ namespace: tool.namespace ?? null, name: tool.name }));
  if (value.selector_usage && typeof value.selector_usage === 'object') {
    result.selector_usage = {};
    for (const field of ['input_tokens', 'output_tokens', 'total_tokens']) {
      if (Number.isFinite(value.selector_usage[field])) result.selector_usage[field] = value.selector_usage[field];
      else if (value.selector_usage[field] === null) result.selector_usage[field] = null;
    }
  }
  return result;
}

export function selectorOutcomes(text) {
  const outcomes = [];
  for (const line of text.replace(/\x1b\[[0-9;]*m/g, '').split('\n')) {
    const marker = line.indexOf('selector_outcome=');
    if (marker < 0) continue;
    const suffix = line.slice(marker + 'selector_outcome='.length).trim();
    try {
      let value;
      if (suffix.startsWith('"')) {
        const quoted = suffix.match(/^"(?:[^"\\]|\\.)*"/);
        value = JSON.parse(JSON.parse(quoted[0]));
      } else {
        const end = suffix.lastIndexOf('}');
        value = JSON.parse(suffix.slice(0, end + 1));
      }
      outcomes.push(safeOutcome(value));
    } catch { /* Invalid/partial trace output cannot prove selector execution. */ }
  }
  return outcomes;
}

export function sanitizedEvent(event, secrets) {
  const clean = { type: event.type };
  if (event.type === 'turn.completed') clean.usage = event.usage;
  if (['error', 'turn.failed'].includes(event.type)) clean.message = redact(event.message ?? event.error?.message ?? '', secrets).slice(0, 1000);
  if (event.item) {
    const item = event.item;
    clean.item = { id: item.id, type: item.type, status: item.status };
    if (item.type === 'agent_message') clean.item.text = redact(item.text, secrets).slice(0, 4000);
    if (item.type === 'command_execution') {
      if (Number.isInteger(item.exit_code)) clean.item.exit_code = item.exit_code;
      // Retain proof of the requested test command, without storing shell text.
      clean.item.python_unit_tests = pythonUnitTestCommand(item.command);
      clean.item.python_unit_tests_passed = clean.item.python_unit_tests
        && /^Ran 2 tests in [^\r\n]+\r?\n\s*\r?\nOK(?:\r?\n|$)/m.test(item.aggregated_output ?? '');
    }
    if (item.type === 'mcp_tool_call') {
      Object.assign(clean.item, { server: item.server, tool: item.tool, arguments: item.arguments });
      if (item.result) clean.item.result = item.result;
      if (item.error) clean.item.error = { message: redact(item.error.message, secrets).slice(0, 500) };
    }
  }
  const sanitize = (value) => {
    if (typeof value === 'string') return redact(value, secrets);
    if (Array.isArray(value)) return value.map(sanitize);
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, nested]) => [key, sanitize(nested)]));
    return value;
  };
  return sanitize(clean);
}

export function pythonUnitTestCommand(command) {
  if (typeof command !== 'string') return false;
  // Native exec JSON can show POSIX shell quoting. Also accept a test-first
  // Python assertion joined by &&: a failing test cannot reach that assertion.
  const wrapper = /^(?:\/[^\s]+\/)?(?:bash|sh|zsh)\s+-[lc]{1,2}\s+(['"])(.*)\1$/s.exec(command.trim());
  let inner = wrapper ? wrapper[2] : command.trim();
  if (wrapper?.[1] === "'") inner = inner.replaceAll("'\"'\"'", "'").replaceAll("'\\''", "'");
  else if (wrapper?.[1] === '"') inner = inner.replace(/\\([$`"\\\n])/g, (_match, character) => character === '\n' ? '' : character);
  const tests = '(?:python3|python)(?:\\s+-I)?\\s+-m\\s+unittest(?:\\s+-q)?';
  const assertion = '(?:python3|python)\\s+-c\\s+(?:"(?:[^"\\\\]|\\\\[\\s\\S])*"|\'[^\']*\')';
  return new RegExp(`^${tests}(?:\\s+&&\\s+${assertion})?$`).test(inner.trim());
}

function minimalEnvironment() {
  const result = {};
  for (const name of ['PATH', 'HOME', 'USER', 'LOGNAME', 'LANG', 'LC_ALL', 'TERM', 'COLORTERM', 'XDG_RUNTIME_DIR', 'SSL_CERT_FILE', 'SSL_CERT_DIR', 'NODE_EXTRA_CA_CERTS', 'TMPDIR']) {
    if (process.env[name]) result[name] = process.env[name];
  }
  return result;
}

function terminateGroup(child, signal = 'SIGTERM') {
  if (!child?.pid) return;
  try { process.kill(-child.pid, signal); } catch (error) { if (error.code !== 'ESRCH') throw error; }
}

async function stopChild(child) {
  if (!child) return;
  child.stdin?.end();
  terminateGroup(child);
  await new Promise((resolve) => setTimeout(resolve, 150));
  terminateGroup(child, 'SIGKILL');
}

async function freePort() {
  const listener = net.createServer();
  await new Promise((resolve, reject) => { listener.once('error', reject); listener.listen(0, '127.0.0.1', resolve); });
  const port = listener.address().port;
  await new Promise((resolve) => listener.close(resolve));
  return port;
}

async function waitForPort(port, child, timeoutMs = 30_000) {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    if (child.exitCode !== null) throw new Error('Bundled Cursor helper exited during startup.');
    const ready = await new Promise((resolve) => {
      const socket = net.connect({ host: '127.0.0.1', port });
      socket.setTimeout(250);
      socket.once('connect', () => { socket.destroy(); resolve(true); });
      socket.once('error', () => { socket.destroy(); resolve(false); });
      socket.once('timeout', () => { socket.destroy(); resolve(false); });
    });
    if (ready) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error('Bundled Cursor helper startup timed out.');
}

async function startCursorHelper(jobDir, helperDir = path.join(repoRoot, 'codex-rs/overmind-cursor/helper')) {
  await access(path.join(helperDir, 'dist/index.js'));
  const port = await freePort();
  const stateDir = path.join(jobDir, 'cursor-helper-state');
  await mkdir(stateDir, { recursive: true, mode: 0o700 });
  const child = spawn(process.execPath, ['overmind-entry.mjs'], {
    cwd: helperDir, detached: true, stdio: ['pipe', 'ignore', 'ignore'],
    env: { ...minimalEnvironment(), HOST: '127.0.0.1', PORT: String(port), AUTH_MODE: 'byok', STATE_DIR: stateDir,
      EMPTY_WORKSPACE_DIR: path.join(stateDir, 'empty-workspace'), MAX_BODY_BYTES: String(64 * 1024 * 1024), GLOBAL_ACTIVE_RUNS: '1',
      PER_CREDENTIAL_ACTIVE_RUNS: '1', HOSTED_SEARCH_MODE: 'auto', OVERMIND_CODEX_COMPAT: '1', LOG_LEVEL: 'error' },
  });
  try { await waitForPort(port, child); } catch (error) { await stopChild(child); throw error; }
  return { child, port, target: `http://127.0.0.1:${port}/v1/` };
}

async function privateWrite(file, value) {
  await writeFile(file, value, { mode: 0o600 });
  await chmod(file, 0o600);
}

export async function prepareFixture({ jobDir, provider, model, arm, ticket, auditFile, observerUrl, selectorEndpoint, selectorTimeoutMs = 2000, coding = false }) {
  const codexHome = path.join(jobDir, 'codex-home');
  const projectDir = path.join(jobDir, 'project');
  const pluginRoot = path.join(codexHome, 'plugins/cache/overmind-benchmark/fixture-tools/local');
  await mkdir(path.join(pluginRoot, '.codex-plugin'), { recursive: true, mode: 0o700 });
  await mkdir(projectDir, { recursive: true, mode: 0o700 });
  await privateWrite(path.join(projectDir, 'README.md'), 'Isolated harmless Overmind validation fixture. No project source, personal memories, or credentials are present.\n');
  await privateWrite(path.join(pluginRoot, '.codex-plugin/plugin.json'), JSON.stringify({ name: 'fixture-tools', version: '1.0.0', description: 'Read-only synthetic shipping, fulfillment, and audio facts for isolated Overmind validation.' }, null, 2));
  await privateWrite(path.join(pluginRoot, '.mcp.json'), JSON.stringify({ mcpServers: { fixture: { command: process.execPath, args: [path.join(scriptDir, 'fixture-mcp.mjs')],
    env_vars: ['OVERMIND_FIXTURE_SEED'], env: { OVERMIND_FIXTURE_TICKET: ticket, OVERMIND_FIXTURE_AUDIT: auditFile } } } }, null, 2));
  let config = `model = ${JSON.stringify(model)}\nmodel_provider = ${JSON.stringify(provider)}\napproval_policy = "never"\nsandbox_mode = "${coding ? 'workspace-write' : 'read-only'}"\nweb_search = "disabled"\nproject_doc_max_bytes = 0\ncli_auth_credentials_store = "ephemeral"\ncheck_for_update_on_startup = false\nsuppress_unstable_features_warning = true\n`;
  if (provider === 'openai') config += `openai_base_url = ${JSON.stringify(observerUrl)}\n`;
  if (coding) config += '\n[sandbox_workspace_write]\nnetwork_access = false\n';
  config += `\n[features]\nplugins = true\nremote_plugin = false\napps = false\nshell_tool = ${coding}\nmulti_agent = false\n`;
  config += '\n[plugins."fixture-tools@overmind-benchmark"]\nenabled = true\n';
  await privateWrite(path.join(codexHome, 'config.toml'), config);
  let overmind = `[tool_selector]\nenabled = ${arm === 'decisions'}\nmodel = "gpt-6-luna"\ntimeout_ms = ${selectorTimeoutMs}\nmax_tools = 8\nmin_probability = 0.55\napi_key_env = "OPENAI_API_KEY"\n`;
  if (selectorEndpoint) overmind += `endpoint = ${JSON.stringify(selectorEndpoint)}\n`;
  await privateWrite(path.join(codexHome, 'overmind.toml'), overmind);
  if (coding) await prepareCodingProject(projectDir);
  return { codexHome, projectDir };
}

export function evaluateResult({ task, ticket, seed, events, audit, selectors, arm, exitCode, timedOut, requestLimit }) {
  const expected = factsFor(seed, ticket);
  const calls = audit.filter((entry) => entry.type === 'tools.call');
  const correctCalls = calls.filter((entry) => entry.ok && entry.valid_ticket && task.requiredTools.includes(entry.name));
  const missing = task.requiredTools.filter((name) => !correctCalls.some((entry) => entry.name === name));
  const unrelated = calls.filter((entry) => !task.requiredTools.includes(entry.name));
  const final = events.filter((event) => event.type === 'item.completed' && event.item?.type === 'agent_message').at(-1)?.item.text ?? '';
  let finalCorrect;
  if (task.fields.length === 1) finalCorrect = final.trim().replace(/^```(?:json)?\s*|\s*```$/g, '').replace(/^"|"$/g, '').trim() === String(expected[task.fields[0]]);
  else {
    try {
      const parsed = JSON.parse(final.trim().replace(/^```(?:json)?\s*|\s*```$/g, ''));
      finalCorrect = task.fields.every((field) => parsed[field] === expected[field]);
    } catch { finalCorrect = false; }
  }
  const selectorProved = arm !== 'decisions' || selectors.some((outcome) => outcome.candidate_count > 0 && outcome.request_count > 0);
  const turnCompleted = events.some((event) => event.type === 'turn.completed');
  return {
    success: exitCode === 0 && !timedOut && !requestLimit && turnCompleted && missing.length === 0 && unrelated.length === 0 && finalCorrect && selectorProved,
    final_correct: finalCorrect, required_tool_calls_proved: missing.length === 0, missing_tools: missing,
    unrelated_tool_calls: unrelated.length, mcp_tool_calls: calls.length,
    selector_execution_proved: selectorProved, turn_completed: turnCompleted,
    expected: Object.fromEntries(task.fields.map((field) => [field, expected[field]])), final_answer: final,
  };
}

export async function runJob(options) {
  const { binary, jobDir, provider, model, arm, ticket, seed, credentials, taskId, timeoutMs = 120_000, maxRequests = 8, selectorEndpoint, selectorTimeoutMs, mockTarget, helperDir } = options;
  if (!allowedProviders.includes(provider) || !allowedArms.includes(arm)) throw new Error('Unsupported provider or arm.');
  if (!Number.isInteger(maxRequests) || maxRequests < 1 || maxRequests > 8 || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 120_000) throw new Error('Jobs require 1–8 Responses requests and a timeout no greater than 120 seconds.');
  await mkdir(jobDir, { recursive: true, mode: 0o700 });
  const auditFile = path.join(jobDir, 'mcp-audit.jsonl');
  const task = taskId === 'coding' ? codingTask(ticket) : taskFor(taskId, ticket);
  let helper = null;
  let observer = null;
  let child = null;
  let timedOut = false;
  let requestLimit = false;
  const secrets = [credentials.openai, credentials.cursor];
  const events = [];
  let trace = '';
  let partialStdout = '';
  const started = performance.now();
  let fixture;
  try {
    if (provider === 'cursor' && !mockTarget) helper = await startCursorHelper(jobDir, helperDir);
    const target = mockTarget ?? helper?.target ?? 'https://api.openai.com/v1/';
    observer = await startObserver({ target, loopbackPort: mockTarget ? Number(new URL(mockTarget).port) : helper?.port,
      maxRequests, requestTimeoutMs: Math.min(timeoutMs, 90_000), onLimit: () => { requestLimit = true; terminateGroup(child); } });
    fixture = await prepareFixture({ jobDir, provider, model, arm, ticket, auditFile, observerUrl: observer.url, selectorEndpoint, selectorTimeoutMs, coding: taskId === 'coding' });
    if (taskId === 'coding') {
      const baseline = await verifyCodingProject(fixture.projectDir, factsFor(seed, ticket).dispatch_code);
      if (baseline.exit_code !== 1 || !baseline.test_fixture_unchanged || baseline.timed_out) throw new Error('Python coding fixture did not show its expected failing baseline before inference.');
    }
    const env = { ...minimalEnvironment(), CODEX_HOME: fixture.codexHome, OPENAI_API_KEY: credentials.openai ?? '', CODEX_API_KEY: credentials.openai ?? '',
      CURSOR_API_KEY: credentials.cursor ?? '', OVERMIND_FIXTURE_SEED: seed,
      RUST_LOG: 'overmind::tool_selector=info,codex_core=warn' };
    if (taskId === 'coding') {
      env.HOME = path.join(jobDir, 'isolated-home');
      env.XDG_CONFIG_HOME = path.join(env.HOME, '.config');
      await mkdir(env.XDG_CONFIG_HOME, { recursive: true, mode: 0o700 });
    }
    if (provider === 'cursor') env.OVERMIND_CURSOR_BASE_URL = observer.url;
    const args = ['exec', '--json', '--ephemeral', '--skip-git-repo-check', '--ignore-rules', '--color', 'never', '-C', fixture.projectDir, '-m', model, task.prompt];
    child = spawn(binary, args, { cwd: fixture.projectDir, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    child.stderr.on('data', (chunk) => { trace += redact(chunk.toString(), secrets); if (trace.length > 2 * 1024 * 1024) trace = trace.slice(-1024 * 1024); });
    const acceptLine = (line) => {
      try {
        const event = sanitizedEvent(JSON.parse(line), secrets);
        events.push(event);
        if (events.filter((entry) => entry.type === 'item.completed' && entry.item?.type === 'mcp_tool_call').length > 4) { requestLimit = true; terminateGroup(child); }
      } catch { /* Only structured exec events are retained. */ }
    };
    child.stdout.on('data', (chunk) => {
      partialStdout += chunk.toString();
      if (partialStdout.length > 4 * 1024 * 1024) { requestLimit = true; terminateGroup(child); partialStdout = ''; return; }
      let end;
      while ((end = partialStdout.indexOf('\n')) >= 0) { acceptLine(partialStdout.slice(0, end)); partialStdout = partialStdout.slice(end + 1); }
    });
    const remainingMs = Math.max(1, timeoutMs - (performance.now() - started));
    const timer = setTimeout(() => { timedOut = true; terminateGroup(child); }, remainingMs);
    const killTimer = setTimeout(() => terminateGroup(child, 'SIGKILL'), remainingMs + 1500);
    let exitCode;
    try {
      exitCode = await new Promise((resolve, reject) => { child.once('error', () => reject(new Error('Built Overmind executable could not start.'))); child.once('close', (code) => resolve(code)); });
    } finally { clearTimeout(timer); clearTimeout(killTimer); }
    if (partialStdout.trim()) acceptLine(partialStdout);
    // Use one trace source. Deduplicating equal outcomes could erase two real
    // equal-cost attempts; combining file and stderr can count one attempt twice.
    let selectors = selectorOutcomes(trace);
    let selectorTraceSource = selectors.length ? 'stderr' : null;
    if (selectors.length === 0) {
      try {
        for (const name of (await readdir(path.join(fixture.codexHome, 'log'))).sort()) {
          if (!/\.log$/.test(name)) continue;
          const outcomes = selectorOutcomes(redact(await readFile(path.join(fixture.codexHome, 'log', name), 'utf8'), secrets));
          if (outcomes.length > selectors.length) { selectors = outcomes; selectorTraceSource = `log/${name}`; }
        }
      } catch { /* No file-backed trace is normal for exec. */ }
    }
    const audit = (await optionalFile(auditFile)).split('\n').filter(Boolean).map((line) => JSON.parse(line));
    const evaluation = evaluateResult({ task, ticket, seed, events, audit, selectors, arm, exitCode, timedOut, requestLimit });
    let codingVerification = null;
    if (taskId === 'coding') {
      codingVerification = await verifyCodingProject(fixture.projectDir, factsFor(seed, ticket).dispatch_code);
      codingVerification.baseline_failed_before_inference = true;
      codingVerification.unit_test_command_proved = events.some((event) => event.type === 'item.completed' && event.item?.type === 'command_execution' && event.item.exit_code === 0 && event.item.python_unit_tests_passed === true);
      evaluation.success &&= codingVerification.passed && codingVerification.unit_test_command_proved;
    }
    const actualSelectorRequests = selectors.filter((value) => value.request_count > 0);
    const selectorTokens = (field) => actualSelectorRequests.every((value) => Number.isFinite(value.selector_usage?.[field]))
      ? actualSelectorRequests.reduce((sum, value) => sum + value.selector_usage[field], 0) : null;
    const summary = {
      provider, model, arm, task: taskId, ticket, elapsed_ms: Math.round(performance.now() - started), exit_code: exitCode,
      timed_out: timedOut, request_limit_reached: requestLimit, ...evaluation,
      selector_outcomes: selectors, selector_trace_source: selectorTraceSource, responses_requests: observer.records.filter((record) => record.kind === 'responses').length,
      tool_search_calls: observer.records.reduce((count, record) => count + record.tool_search_calls + (record.nested_tool_search_calls ?? 0), 0),
      catalog_discovery_attempts: observer.records.reduce((count, record) => count + (record.catalog_discovery_attempts ?? 0), 0),
      transport_usage: usageSummary(observer.records, provider), exec_usage: events.findLast((event) => event.type === 'turn.completed')?.usage ?? null,
      selector_requests: actualSelectorRequests.reduce((sum, value) => sum + value.request_count, 0),
      selector_input_tokens: selectorTokens('input_tokens'), selector_output_tokens: selectorTokens('output_tokens'), selector_total_tokens: selectorTokens('total_tokens'),
      selector_usage_complete: actualSelectorRequests.every((value) => ['selected', 'empty'].includes(value.status)
        && ['input_tokens', 'output_tokens', 'total_tokens'].every((field) => Number.isFinite(value.selector_usage?.[field]))),
      ...(codingVerification ? { coding_verification: codingVerification } : {}),
      startup_diagnostic: observer.records.length === 0 && events.length === 0 ? redact(trace, secrets).slice(0, 2000) : null,
      transport: 'http_fallback',
      transport_detail: 'Native provider through a loopback observer; GET Responses returns426 for immediate native HTTP fallback. POST requests forwarded unchanged to official OpenAI or bundled Cursor helper.',
    };
    await privateWrite(path.join(jobDir, 'events.jsonl'), events.map((event) => JSON.stringify(event)).join('\n') + '\n');
    await privateWrite(path.join(jobDir, 'requests.json'), JSON.stringify(observer.records, null, 2) + '\n');
    await privateWrite(path.join(jobDir, 'summary.json'), JSON.stringify(summary, null, 2) + '\n');
    return summary;
  } finally {
    await stopChild(child);
    await observer?.close();
    await stopChild(helper?.child);
  }
}

async function binaryHash(binary) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(binary)) hash.update(chunk);
  return hash.digest('hex');
}

export function buildPlan({ providers = allowedProviders, arms = allowedArms, tasks = TASK_IDS, reps = 1, maxJobs = 24 }) {
  if (!Number.isInteger(reps) || reps < 1 || reps > 6) throw new Error('Repetitions must be an integer between 1 and 6.');
  if (![providers, arms, tasks].every((values) => values.length > 0 && new Set(values).size === values.length) || !Number.isInteger(maxJobs) || maxJobs < 1 || maxJobs > 24) throw new Error('Plan lists must be nonempty and unique; maxJobs must be 1–24.');
  if (providers.some((value) => !allowedProviders.includes(value)) || arms.some((value) => !allowedArms.includes(value)) || tasks.some((value) => ![...TASK_IDS, 'coding'].includes(value))) throw new Error('Unknown provider, arm, or task.');
  const plan = [];
  for (let rep = 0; rep < reps; rep += 1) for (const task of tasks) for (const provider of providers) {
    const pair = `${rep + 1}-${task}-${provider}`;
    const ticket = `TICKET-${randomBytes(8).toString('hex')}`;
    const seed = randomBytes(24).toString('hex');
    for (const arm of rep % 2 ? [...arms].reverse() : arms) plan.push({ rep: rep + 1, pair, task, provider, arm, ticket, seed });
  }
  if (plan.length > Math.min(maxJobs, 24)) throw new Error(`Plan would run ${plan.length} jobs; hard limit is ${Math.min(maxJobs, 24)}.`);
  return plan;
}

export function pairedSummary(results) {
  const pairs = [];
  for (const key of new Set(results.map((result) => result.pair))) {
    const plain = results.find((result) => result.pair === key && result.arm === 'plain');
    const decisions = results.find((result) => result.pair === key && result.arm === 'decisions');
    if (!plain || !decisions) continue;
    const delta = (field) => Number.isFinite(plain[field]) && Number.isFinite(decisions[field]) ? decisions[field] - plain[field] : null;
    const usageDelta = (field) => plain.transport_usage.complete && decisions.transport_usage.complete && Number.isFinite(plain.transport_usage[field]) && Number.isFinite(decisions.transport_usage[field]) ? decisions.transport_usage[field] - plain.transport_usage[field] : null;
    pairs.push({ pair: key, provider: plain.provider, task: plain.task, both_correct: plain.success && decisions.success,
      decisions_minus_plain: { elapsed_ms: delta('elapsed_ms'), responses_requests: delta('responses_requests'), tool_search_calls: delta('tool_search_calls'),
        catalog_discovery_attempts: delta('catalog_discovery_attempts'),
        input_tokens: usageDelta('input_tokens'), cached_input_tokens: usageDelta('cached_input_tokens'), uncached_input_tokens: usageDelta('uncached_input_tokens'),
        output_tokens: usageDelta('output_tokens'), selector_input_tokens: decisions.selector_input_tokens },
    });
  }
  return pairs;
}

function argumentsFor(argv) {
  const result = { command: argv[0] ?? 'preflight' };
  for (let index = 1; index < argv.length; index += 1) {
    const name = argv[index];
    if (!name.startsWith('--')) throw new Error('Expected a named option.');
    if (['--continue-on-failure', '--confirm-live'].includes(name)) result[name.slice(2)] = true;
    else { if (!argv[index + 1] || argv[index + 1].startsWith('--')) throw new Error(`Missing value for ${name}.`); result[name.slice(2)] = argv[++index]; }
  }
  return result;
}

async function main() {
  const args = argumentsFor(process.argv.slice(2));
  const sourceHome = path.resolve(args['source-home'] ?? process.env.CODEX_HOME ?? path.join(homedir(), '.codex'));
  const credentials = await readCredentials(sourceHome);
  const binary = path.resolve(args.binary ?? path.join(repoRoot, 'codex-rs/target/debug/codex'));
  const helperDir = path.resolve(args['helper-dir'] ?? path.join(repoRoot, 'codex-rs/overmind-cursor/helper'));
  const helperBuilt = await access(path.join(helperDir, 'dist/index.js')).then(() => true, () => false);
  const binaryPresent = await access(binary).then(() => true, () => false);
  if (args.command === 'preflight') {
    console.log(JSON.stringify({ openai_api_key_present: Boolean(credentials.openai), cursor_api_key_present: Boolean(credentials.cursor), helper_built: helperBuilt, binary_present: binaryPresent, inference_requests: 0 }));
    return;
  }
  if (args.command !== 'run') throw new Error('Supported commands: preflight, run. Offline validation uses offline.mjs.');
  if (!args['confirm-live']) throw new Error('Live execution requires --confirm-live after the controlled run is scheduled.');
  if (!args.binary) throw new Error('Live execution requires the explicit path to the freshly built Overmind binary.');
  const providers = args.providers?.split(',') ?? allowedProviders;
  if (providers.includes('openai') && !credentials.openai || providers.includes('cursor') && !credentials.cursor || (args.arms ?? 'plain,decisions').includes('decisions') && !credentials.openai) throw new Error('Required provider/selector credential is absent; preflight reports presence without values.');
  const plan = buildPlan({ providers, arms: args.arms?.split(',') ?? allowedArms, tasks: args.tasks?.split(',') ?? TASK_IDS, reps: Number(args.reps ?? 1), maxJobs: Number(args['max-jobs'] ?? 24) });
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const output = path.resolve(args.output ?? path.join(repoRoot, `.codex/specs/decisions-tui/live-${timestamp}`));
  await mkdir(path.dirname(output), { recursive: true, mode: 0o700 });
  await mkdir(output, { recursive: false, mode: 0o700 });
  const metadata = { started_at: new Date().toISOString(), binary, binary_sha256: await binaryHash(binary), helper_directory: helperDir, planned_jobs: plan.length,
    transport: 'http_fallback',
    openai_model: args['openai-model'] ?? 'gpt-6.1-sol', cursor_model: args['cursor-model'] ?? 'composer-2.5', reps: Number(args.reps ?? 1),
    max_responses_requests_per_job: Number(args['max-requests'] ?? 8), timeout_ms_per_job: Number(args['timeout-ms'] ?? 120_000),
    plan: plan.map(({ seed: _seed, ...entry }) => entry),
    caveats: ['Small paired fixture study, not a production-quality or general speed claim.', 'Native provider requests pass through a local observer; credentials and raw request/response bodies are never persisted.', 'Observation uses HTTP fallback: GET Responses returns426 immediately; WebSocket performance is not measured.', 'Provider model metadata determines native tool mode; no mode override is applied.', 'Cursor SDK input, cache read, and cache creation fields are additive. Normalized total input is their sum; uncached input is SDK input plus cache creation. Missing components remain null.', 'API usage and timing depend on model behavior and caching. Selector usage is reported separately; missing usage remains null.'] };
  await privateWrite(path.join(output, 'manifest.json'), JSON.stringify(metadata, null, 2) + '\n');
  const results = [];
  for (const [index, entry] of plan.entries()) {
    const name = `${String(index + 1).padStart(2, '0')}-${entry.provider}-${entry.arm}-${entry.task}-r${entry.rep}`;
    console.log(JSON.stringify({ state: 'starting', job: name, index: index + 1, total: plan.length }));
    const result = await runJob({ binary, jobDir: path.join(output, name), provider: entry.provider, arm: entry.arm, taskId: entry.task,
      ticket: entry.ticket, seed: entry.seed, credentials, model: entry.provider === 'openai' ? metadata.openai_model : metadata.cursor_model,
      timeoutMs: metadata.timeout_ms_per_job, maxRequests: metadata.max_responses_requests_per_job, helperDir });
    results.push({ ...result, pair: entry.pair, rep: entry.rep });
    await privateWrite(path.join(output, 'results.json'), JSON.stringify({ metadata, results, pairs: pairedSummary(results) }, null, 2) + '\n');
    console.log(JSON.stringify({ state: 'completed', job: name, success: result.success, elapsed_ms: result.elapsed_ms, responses_requests: result.responses_requests, mcp_tool_calls: result.mcp_tool_calls }));
    if (!result.success && !args['continue-on-failure']) { process.exitCode = 1; break; }
  }
  console.log(JSON.stringify({ output, jobs_completed: results.length, all_correct: results.length === plan.length && results.every((result) => result.success), pairs: pairedSummary(results) }));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch((error) => { console.error(redact(error.message)); process.exitCode = 1; });
