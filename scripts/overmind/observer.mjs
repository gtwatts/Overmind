// A loopback measurement proxy. It never records headers, bodies, or raw SSE.
import http from 'node:http';
import https from 'node:https';
import { brotliDecompressSync, gunzipSync, inflateSync } from 'node:zlib';
import { TOOLS } from './fixture-mcp.mjs';

const fixtureNames = new Set(TOOLS.map((tool) => tool.name));
const usageFields = [
  'input_tokens', 'output_tokens', 'total_tokens', 'cache_read_input_tokens',
  'cache_creation_input_tokens', 'run_input_tokens', 'run_output_tokens',
  'run_cache_read_input_tokens', 'run_cache_creation_input_tokens', 'run_reasoning_output_tokens', 'model_steps',
];

export function safeUsage(value) {
  if (!value || typeof value !== 'object') return null;
  const result = {};
  for (const field of usageFields) {
    if (Number.isFinite(value[field]) && value[field] >= 0) result[field] = value[field];
  }
  if (Number.isFinite(value.input_tokens_details?.cached_tokens)) result.cached_input_tokens = value.input_tokens_details.cached_tokens;
  if (Number.isFinite(value.output_tokens_details?.reasoning_tokens)) result.reasoning_output_tokens = value.output_tokens_details.reasoning_tokens;
  if (['sdk', 'unavailable', 'deferred'].includes(value.usage_status)) result.usage_status = value.usage_status;
  if (value.usage_deferred === true) result.usage_deferred = true;
  return result;
}

export function toolInventory(tools) {
  const direct = [];
  const deferred = [];
  const codeMode = [];
  let exec = false;
  let search = false;
  const visit = (tool, inheritedDeferred = false) => {
    if (!tool || typeof tool !== 'object') return;
    const isDeferred = inheritedDeferred || tool.defer_loading === true;
    if (tool.type === 'tool_search') search = true;
    if (tool.type === 'custom' && tool.name === 'exec') {
      exec = true;
      // Only full declaration headings prove preload; metadata or guidance may
      // mention a deferred tool without exposing its schema to the model.
      const headings = [...String(tool.description ?? '').matchAll(/^### `([^`]+)`/gm)].map((match) => match[1]);
      for (const name of fixtureNames) if (headings.includes(`mcp__fixture__${name}`)) codeMode.push(name);
      if (headings.includes('tool_search')) search = true;
    }
    if (tool.type === 'namespace') {
      for (const nested of tool.tools ?? []) visit(nested, isDeferred);
    } else if (fixtureNames.has(tool.name)) {
      (isDeferred ? deferred : direct).push(tool.name);
    }
  };
  for (const tool of tools ?? []) visit(tool);
  return { direct_fixture_tools: [...new Set(direct)].sort(), deferred_fixture_tools: [...new Set(deferred)].sort(),
    code_mode_fixture_tools: [...new Set(codeMode)].sort(), code_mode_exec_available: exec, tool_search_available: search };
}

export function requestToolInventory(body) {
  // Native Responses Lite carries declarations in developer additional_tools
  // items instead of the top-level tools field. Never inspect message text.
  const tools = Array.isArray(body.tools) ? [...body.tools] : [];
  for (const item of Array.isArray(body.input) ? body.input : []) {
    if (item.type === 'additional_tools' && Array.isArray(item.tools)) tools.push(...item.tools);
  }
  return toolInventory(tools);
}

export function nestedToolMeasurements(input, seen = new Set()) {
  let searches = 0;
  const tools = [];
  for (const item of input ?? []) {
    const metadata = item.internal_chat_message_metadata_passthrough;
    for (const [index, call] of (metadata?.executed_tool_calls ?? []).entries()) {
      const name = typeof call.name === 'string' ? call.name : call.name?.name;
      const identity = `${item.call_id ?? ''}:${metadata.cell_id ?? ''}:${index}:${name}`;
      if (seen.has(identity)) continue;
      seen.add(identity);
      if (name === 'tool_search') searches += 1;
      for (const fixture of fixtureNames) {
        if ([fixture, `mcp__fixture__${fixture}`, `mcp__fixture.${fixture}`].includes(name)) tools.push(fixture);
      }
    }
  }
  return { nested_tool_search_calls: searches, executed_fixture_tools: [...new Set(tools)].sort() };
}

export function sseUsageCollector(record) {
  let pending = '';
  let data = [];
  const event = () => {
    if (!data.length) return;
    try {
      const value = JSON.parse(data.join('\n'));
      if (value.type === 'response.output_item.done') {
        if (value.item?.type === 'tool_search_call') record.tool_search_calls += 1;
        if (['function_call', 'custom_tool_call'].includes(value.item?.type)) record.function_calls += 1;
        if (value.item?.type === 'custom_tool_call' && value.item.name === 'exec' && /\bALL_TOOLS\b/.test(value.item.input ?? '')) {
          record.catalog_discovery_attempts = (record.catalog_discovery_attempts ?? 0) + 1;
        }
        if (value.item?.type === 'message' && value.item?.role === 'assistant') record.has_agent_message = true;
      }
      if (['response.completed', 'response.done', 'response.incomplete'].includes(value.type)) {
        record.usage = safeUsage(value.response?.usage);
        record.completed = value.type !== 'response.incomplete';
        if (value.response?.output?.some((item) => item.type === 'message' && item.role === 'assistant')) record.has_agent_message = true;
      }
    } catch { /* Non-JSON SSE such as [DONE] carries no measurement. */ }
    data = [];
  };
  return {
    write(chunk) {
      pending += chunk.toString('utf8');
      if (pending.length > 2 * 1024 * 1024) { pending = ''; data = []; record.measurement_truncated = true; return; }
      let newline;
      while ((newline = pending.indexOf('\n')) >= 0) {
        const line = pending.slice(0, newline).replace(/\r$/, '');
        pending = pending.slice(newline + 1);
        if (line === '') event();
        else if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
      }
    },
    end() { event(); pending = ''; },
  };
}

function decodedJson(buffer, encoding) {
  let bytes = buffer;
  if (encoding === 'gzip') bytes = gunzipSync(buffer);
  else if (encoding === 'br') bytes = brotliDecompressSync(buffer);
  else if (encoding === 'deflate') bytes = inflateSync(buffer);
  return JSON.parse(bytes.toString('utf8'));
}

export function validateDestination(target, { loopbackPort = null } = {}) {
  const destination = new URL(target);
  if (destination.username || destination.password || destination.search || destination.hash) throw new Error('Observer destination cannot contain credentials, query, or fragment.');
  const official = destination.href === 'https://api.openai.com/v1/';
  const ownedLoopback = destination.protocol === 'http:' && destination.hostname === '127.0.0.1' && Number(destination.port) === loopbackPort && destination.pathname === '/v1/';
  if (!official && !ownedLoopback) throw new Error('Observer destination must be official OpenAI or the explicitly owned loopback fixture/helper.');
  return destination;
}

export async function startObserver({ target, loopbackPort = null, maxRequests = 8, requestTimeoutMs = 90_000, onLimit = () => {} }) {
  const destination = validateDestination(target, { loopbackPort });
  const records = [];
  const active = new Set();
  const seenNestedCalls = new Set();
  let responseRequests = 0;
  let websocketAttempts = 0;
  let closed = false;
  const server = http.createServer(async (request, response) => {
    const path = (request.url ?? '').split('?')[0];
    if (request.method === 'GET' && path === '/v1/responses') {
      if (++websocketAttempts > maxRequests) { onLimit(); response.writeHead(429); response.end(); return; }
      records.push({ request: records.length + 1, kind: 'websocket_fallback', status: 426, elapsed_ms: 0, tool_search_calls: 0, catalog_discovery_attempts: 0 });
      response.writeHead(426, { 'content-type': 'application/json', connection: 'close' });
      response.end(JSON.stringify({ error: { message: 'Fixture observer uses HTTP Responses fallback.', type: 'fixture_http_fallback' } }));
      return;
    }
    const isResponse = request.method === 'POST' && path === '/v1/responses';
    const isModels = request.method === 'GET' && path === '/v1/models';
    if (!isResponse && !isModels) { response.writeHead(404); response.end('Unsupported observer path'); return; }
    if (isResponse && ++responseRequests > maxRequests) {
      response.writeHead(429, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ error: { message: 'Fixture request limit reached', type: 'fixture_limit' } }));
      onLimit();
      return;
    }
    const record = { request: records.length + 1, kind: isResponse ? 'responses' : 'models', status: null, elapsed_ms: null, tool_search_calls: 0, catalog_discovery_attempts: 0, function_calls: 0, has_agent_message: false, completed: false, usage: null };
    records.push(record);
    const start = performance.now();
    const chunks = [];
    let length = 0;
    for await (const chunk of request) {
      length += chunk.length;
      if (length > 16 * 1024 * 1024) { response.writeHead(413); response.end('Fixture body limit reached'); return; }
      chunks.push(chunk);
    }
    const body = Buffer.concat(chunks);
    record.request_bytes = length;
    if (isResponse) {
      try {
        const parsed = decodedJson(body, request.headers['content-encoding']);
        Object.assign(record, requestToolInventory(parsed));
        Object.assign(record, nestedToolMeasurements(parsed.input, seenNestedCalls));
      } catch { record.request_inventory_unavailable = true; }
    }
    const headers = { ...request.headers, host: destination.host, 'accept-encoding': 'identity' };
    delete headers.connection;
    const transport = destination.protocol === 'https:' ? https : http;
    const query = new URL(request.url, 'http://127.0.0.1').search;
    const upstream = transport.request(new URL(`${isResponse ? 'responses' : 'models'}${query}`, destination), { method: request.method, headers }, (incoming) => {
      record.status = incoming.statusCode;
      response.writeHead(incoming.statusCode ?? 502, incoming.headers);
      const collector = sseUsageCollector(record);
      incoming.on('data', (chunk) => { collector.write(chunk); response.write(chunk); });
      incoming.on('end', () => { collector.end(); record.elapsed_ms = Math.round(performance.now() - start); response.end(); active.delete(upstream); });
      incoming.on('error', () => { record.transport_error = true; response.destroy(); active.delete(upstream); });
    });
    active.add(upstream);
    upstream.setTimeout(requestTimeoutMs, () => upstream.destroy(new Error('Fixture request timeout')));
    upstream.on('error', () => {
      record.transport_error = true;
      record.elapsed_ms = Math.round(performance.now() - start);
      active.delete(upstream);
      if (!response.headersSent) response.writeHead(502, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ error: { message: 'Observer upstream transport failed', type: 'fixture_transport_error' } }));
    });
    response.on('close', () => { if (!response.writableEnded) upstream.destroy(); });
    upstream.end(body);
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  return {
    url: `http://127.0.0.1:${server.address().port}/v1`,
    records,
    async close() {
      if (closed) return;
      closed = true;
      for (const request of active) request.destroy();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

export function usageSummary(records, provider) {
  const responses = records.filter((record) => record.kind === 'responses');
  const supplied = responses.filter((record) => record.usage && !record.usage.usage_deferred && !['unavailable', 'deferred'].includes(record.usage.usage_status));
  const sum = (rows, field) => rows.every((row) => Number.isFinite(row[field])) ? rows.reduce((total, row) => total + row[field], 0) : null;
  if (provider !== 'cursor') {
    const usages = supplied.map((record) => record.usage);
    const input = usages.length ? sum(usages, 'input_tokens') : null;
    const cached = usages.length ? sum(usages, 'cached_input_tokens') : null;
    return {
      source: 'provider_responses_usage', complete: responses.length > 0 && supplied.length === responses.length
        && responses.every((record) => record.completed) && [input, cached, usages.length ? sum(usages, 'output_tokens') : null].every(Number.isFinite),
      input_tokens: input, cached_input_tokens: cached,
      uncached_input_tokens: input !== null && cached !== null ? Math.max(0, input - cached) : null,
      output_tokens: usages.length ? sum(usages, 'output_tokens') : null,
    };
  }
  // Cursor exposes SDK totals at the end of a run, and estimates between calls.
  // Never add estimates to a final run total or label missing usage as zero.
  const finals = supplied.filter((record) => record.has_agent_message && Number.isFinite(record.usage.run_input_tokens) && Number.isFinite(record.usage.run_output_tokens));
  const raw = finals.map(({ usage }) => ({
    input_tokens: usage.run_input_tokens,
    output_tokens: usage.run_output_tokens,
    cached_input_tokens: usage.run_cache_read_input_tokens,
    cache_write_input_tokens: usage.run_cache_creation_input_tokens,
    reasoning_output_tokens: usage.run_reasoning_output_tokens,
    model_steps: Number.isInteger(usage.model_steps) && usage.model_steps > 0 ? usage.model_steps : 0,
  }));
  const covered = raw.reduce((total, usage) => total + usage.model_steps, 0);
  const input = raw.length ? sum(raw, 'input_tokens') : null;
  const cached = raw.length ? sum(raw, 'cached_input_tokens') : null;
  const writes = raw.length ? sum(raw, 'cache_write_input_tokens') : null;
  const output = raw.length ? sum(raw, 'output_tokens') : null;
  return {
    source: 'cursor_sdk_completed_run_totals', complete: responses.length > 0 && responses.every((record) => record.completed)
      && covered >= responses.length && [input, cached, writes, output].every(Number.isFinite),
    covered_model_steps: covered,
    sdk_input_tokens: input, sdk_cache_read_input_tokens: cached, sdk_cache_creation_input_tokens: writes,
    input_tokens: input !== null && cached !== null && writes !== null ? input + cached + writes : null,
    cached_input_tokens: cached,
    uncached_input_tokens: input !== null && writes !== null ? input + writes : null,
    output_tokens: output,
    sdk_reasoning_output_tokens: raw.length ? sum(raw, 'reasoning_output_tokens') : null,
    cache_write_input_tokens: writes,
    responses_with_deferred_or_unavailable_usage: responses.length - supplied.length,
    note: 'Installed @cursor/sdk toTokenUsage/sumTokenUsage defines input, cache reads, and cache writes as additive. Total input = SDK input + reads + writes; uncached input = SDK input + writes. Intermediate Responses estimates are excluded. Missing fields stay null; interrupted runs can make totals partial.',
  };
}
