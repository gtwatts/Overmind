#!/usr/bin/env node
// Harmless read-only plugin fixture for Overmind's native MCP/tool-discovery path.
import { createHash } from 'node:crypto';
import { appendFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const definitions = [
  ['shipping_dispatch', 'Look up the shipping dispatch_code for a fixture ticket. Does not return inventory, delivery dates, or audio settings.'],
  ['inventory_available', 'Read the available_units inventory count for a fixture ticket. Pair with delivery_promise when fulfillment needs both stock and a promised delivery.'],
  ['delivery_promise', 'Look up the promised_delivery schedule for a fixture ticket. Does not return available inventory units or a shipping dispatch code.'],
  ['podcast_mastering_profile', 'Read target_lufs, the podcast mastering audio loudness target, for a fixture ticket. This is the audio delivery profile, unrelated to shipping or visual color profiles.'],
  ['progress_delayed_report', 'Read a harmless fixture report after a two-second wait. Emits MCP progress notifications when the client supplies a progress token. Useful for testing live TUI progress.'],
  ['tax_delivery_schedule', 'Read the quarterly tax filing delivery schedule. Financial compliance only; does not provide fulfillment delivery promises.'],
  ['render_delivery_profile', 'Read a video render delivery profile: frame rate and codec. Does not provide podcast loudness or parcel delivery schedules.'],
  ['image_color_profile', 'Read a raster image color profile. Does not provide audio mastering loudness.'],
  ['warehouse_layout', 'Read a warehouse floor layout. Does not provide ticket inventory counts.'],
  ['calendar_meeting', 'Read a meeting calendar entry. Does not provide ticket shipping or delivery facts.'],
  ['dns_record', 'Read a synthetic DNS record. No actual network lookup is performed.'],
  ['database_schema', 'Read a synthetic database table schema. Does not execute queries.'],
  ['git_branch_summary', 'Read a synthetic Git branch summary. Does not access a checkout.'],
  ['video_caption_style', 'Read a synthetic video caption style. Does not provide audio loudness.'],
  ['music_tempo', 'Read a synthetic music tempo. Does not provide podcast mastering loudness.'],
  ['lighting_preset', 'Read a synthetic lighting preset for a render.'],
  ['invoice_balance', 'Read a synthetic invoice balance. Does not provide inventory counts.'],
  ['weather_snapshot', 'Read a fixed synthetic weather snapshot. No external service is called.'],
  ['travel_itinerary', 'Read a synthetic travel itinerary. Does not provide shipping schedules.'],
  ['document_template', 'Read a synthetic document template identifier.'],
  ['support_priority', 'Read a synthetic support priority. Does not provide fulfillment facts.'],
  ['package_version', 'Read a synthetic software package version. No package registry is contacted.'],
  ['stream_resolution', 'Read a synthetic livestream video resolution. Does not provide audio loudness.'],
  ['project_status', 'Read a synthetic project status. Does not provide any shipping, inventory, or audio facts.'],
];

export const TOOLS = definitions.map(([name, description]) => ({
  name,
  description,
  inputSchema: {
    type: 'object',
    properties: { ticket: { type: 'string', description: 'The fixture ticket given by the user.' } },
    required: ['ticket'],
    additionalProperties: false,
  },
  annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
}));

export const TASK_IDS = ['single', 'complementary', 'misleading'];

export function factsFor(seed, ticket) {
  const hex = createHash('sha256').update(`${seed}\0${ticket}`).digest('hex');
  return {
    dispatch_code: `DISP-${hex.slice(0, 12).toUpperCase()}`,
    available_units: 37 + (Number.parseInt(hex.slice(12, 16), 16) % 400),
    promised_delivery: `WINDOW-${hex.slice(16, 28).toUpperCase()}`,
    target_lufs: `-${(12 + (Number.parseInt(hex.slice(28, 32), 16) % 1200) / 100).toFixed(2)} LUFS`,
    report_code: `REPORT-${hex.slice(32, 44).toUpperCase()}`,
  };
}

export function taskFor(id, ticket) {
  const common = `Fixture ticket: ${ticket}. Use the installed read-only fixture plugin to obtain the answer. The facts are generated privately for this ticket, so guessing cannot satisfy this request. Do not run shell commands, edit files, browse, or use unrelated tools. `;
  const tasks = {
    single: {
      requiredTools: ['shipping_dispatch'],
      fields: ['dispatch_code'],
      prompt: `${common}Look up its shipping dispatch_code. Return only the dispatch_code value.`,
    },
    complementary: {
      requiredTools: ['inventory_available', 'delivery_promise'],
      fields: ['available_units', 'promised_delivery'],
      prompt: `${common}Look up both the available_units inventory count and the promised_delivery schedule. Use the two complementary lookups. Return only a JSON object with available_units and promised_delivery.`,
    },
    misleading: {
      requiredTools: ['podcast_mastering_profile'],
      fields: ['target_lufs'],
      prompt: `${common}Read target_lufs from the podcast mastering audio delivery profile. This is a loudness target; video delivery profiles, visual color profiles, and tax or parcel delivery schedules are unrelated. Return only target_lufs including its unit.`,
    },
    progress: {
      requiredTools: ['progress_delayed_report'],
      fields: ['report_code'],
      prompt: `${common}Use progress_delayed_report to read the two-second report. Return only report_code.`,
    },
  };
  if (!tasks[id]) throw new Error(`Unknown fixture task: ${id}`);
  return tasks[id];
}

export function toolResult(name, ticket, seed) {
  const facts = factsFor(seed, ticket);
  const fields = {
    shipping_dispatch: ['dispatch_code'],
    inventory_available: ['available_units'],
    delivery_promise: ['promised_delivery'],
    podcast_mastering_profile: ['target_lufs'],
    progress_delayed_report: ['report_code'],
  };
  return {
    ticket,
    ...(fields[name]
      ? Object.fromEntries(fields[name].map((field) => [field, facts[field]]))
      : { synthetic_fact: `unrelated:${name}` }),
  };
}

export async function serveFixture({ input = process.stdin, output = process.stdout, env = process.env } = {}) {
  const seed = env.OVERMIND_FIXTURE_SEED;
  const ticket = env.OVERMIND_FIXTURE_TICKET;
  if (!seed || !ticket) throw new Error('Fixture seed and ticket must be provided in the process environment.');
  const send = (message) => output.write(`${JSON.stringify(message)}\n`);
  const log = (value) => {
    if (env.OVERMIND_FIXTURE_AUDIT) appendFileSync(env.OVERMIND_FIXTURE_AUDIT, `${JSON.stringify(value)}\n`, { mode: 0o600 });
  };
  const handle = async (message) => {
    const { id, method, params = {} } = message;
    if (id === undefined) return;
    const respond = (result) => send({ jsonrpc: '2.0', id, result });
    if (method === 'initialize') {
      respond({ protocolVersion: params.protocolVersion ?? '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'overmind-fixture', version: '1.0.0' } });
    } else if (method === 'ping') {
      respond({});
    } else if (method === 'tools/list') {
      log({ type: 'tools.list', count: TOOLS.length });
      respond({ tools: TOOLS });
    } else if (method === 'tools/call') {
      const known = TOOLS.some((tool) => tool.name === params.name);
      const validTicket = params.arguments?.ticket === ticket;
      if (!known || !validTicket) {
        log({ type: 'tools.call', name: known ? params.name : 'unknown', valid_ticket: validTicket, ok: false });
        respond({ content: [{ type: 'text', text: 'Unknown tool or invalid fixture ticket.' }], isError: true });
        return;
      }
      if (params.name === 'progress_delayed_report') {
        for (let progress = 1; progress <= 4; progress += 1) {
          await new Promise((resolve) => setTimeout(resolve, 500));
          const progressToken = params._meta?.progressToken;
          if (progressToken !== undefined) {
            send({ jsonrpc: '2.0', method: 'notifications/progress', params: { progressToken, progress, total: 4, message: `Preparing fixture report ${progress}/4` } });
            log({ type: 'progress', progress, total: 4 });
          }
        }
      }
      const result = toolResult(params.name, ticket, seed);
      log({ type: 'tools.call', name: params.name, valid_ticket: true, ok: true, result });
      respond({ content: [{ type: 'text', text: JSON.stringify(result) }], structuredContent: result, isError: false });
    } else if (method === 'resources/list') {
      respond({ resources: [] });
    } else if (method === 'resources/templates/list') {
      respond({ resourceTemplates: [] });
    } else if (method === 'prompts/list') {
      respond({ prompts: [] });
    } else {
      send({ jsonrpc: '2.0', id, error: { code: -32601, message: 'Method not found' } });
    }
  };
  const lines = createInterface({ input });
  for await (const line of lines) {
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    await handle(message);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  serveFixture().catch(() => { process.exitCode = 1; });
}
