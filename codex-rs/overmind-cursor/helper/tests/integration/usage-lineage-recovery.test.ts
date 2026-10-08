import { afterEach, expect, test } from "vitest";
import type { SdkUsage } from "../../src/sdk/port.js";
import { api, closeTestApp, startTestApp, type TestContext } from "../helpers/app.js";

let ctx: TestContext | undefined;
afterEach(async () => {
  if (ctx) await closeTestApp(ctx);
  ctx = undefined;
});

const tools = [{ type: "tool_search", execution: "client" }];
const user = { role: "user", content: "dispatch parcel" };
const plugin = {
  type: "namespace", name: "mcp__fixture", tools: [{
    type: "function", name: "dispatch", description: "Dispatch a parcel.",
    parameters: { type: "object", properties: { parcel: { type: "string" } }, required: ["parcel"] },
  }],
};
const finalUsage: SdkUsage = {
  inputTokens: 11, outputTokens: 5, cacheReadTokens: 2, cacheWriteTokens: 4, reasoningTokens: 3,
};
const retiredUsage: SdkUsage = {
  inputTokens: 7, outputTokens: 3, cacheReadTokens: 1, cacheWriteTokens: 2, reasoningTokens: 1,
};

async function openSearch() {
  ctx = await startTestApp({ sdk: {
    finalUsage,
    agentScripts: [
      [[{ type: "tools", calls: [{ name: "tool_search", input: { query: "dispatch" } }] }]],
      [[
        { type: "tools", calls: [{ name: "tool_search", input: { query: "dispatch" } }] },
        { type: "tools", calls: [{ name: "mcp__fixture__dispatch", input: { parcel: "p1" } }] },
        { type: "text", chunks: ["dispatched"] },
      ], [{ type: "text", chunks: ["next user turn"] }]],
    ],
  }, config: { runtimeLedgerV2: true } });
  const response = await api(ctx, "/v1/responses", {
    method: "POST", body: JSON.stringify({ model: "composer-2.5", tools, input: [user] }),
  });
  expect(response.status).toBe(200);
  const body = await response.json() as { cursor_session_id: string; output: Array<Record<string, unknown>> };
  const search = body.output.find((item) => item.type === "tool_search_call")!;
  expect(search).toBeTruthy();
  const run = ctx.sdk.agents[0]!.runs[0]!;
  const wait = run.wait.bind(run);
  // The SDK's terminal flush can expose usage only after cancel() has returned.
  run.wait = async () => {
    const result = await wait();
    await new Promise((resolve) => setTimeout(resolve, 20));
    return { ...result, usage: retiredUsage };
  };
  const input = [user, search, {
    type: "tool_search_output", call_id: search.call_id, status: "completed", tools: [plugin],
  }];
  const oldSession = ctx.app.registry.get(body.cursor_session_id)!;
  return { run, input, oldSession };
}

async function continueSearch(input: Array<Record<string, unknown>>) {
  const response = await api(ctx!, "/v1/responses", {
    method: "POST", body: JSON.stringify({ model: "composer-2.5", tools, input }),
  });
  expect(response.status).toBe(200);
  return await response.json() as {
    cursor_session_id: string; output: Array<Record<string, unknown>>; usage: Record<string, unknown>;
  };
}

async function finish(input: Array<Record<string, unknown>>, output: Array<Record<string, unknown>>) {
  const call = output.find((item) => item.type === "function_call")!;
  expect(call).toBeTruthy();
  return continueSearch([...input, ...output, { type: "function_call_output", call_id: call.call_id, output: "ok" }]);
}

test("keeps exact cancelled-run usage across native tool_search catalog replacement and duplicate retries", async () => {
  const { run, input, oldSession } = await openSearch();
  const recovered = await Promise.all([continueSearch(input), continueSearch(input), continueSearch(input)]);
  expect(ctx!.sdk.agents).toHaveLength(2);
  expect(recovered[0]!.output).toEqual(recovered[1]!.output);
  const final = await finish(input, recovered[0]!.output);
  expect(final.usage.lineage_usage).toEqual({
    input_tokens: 18, output_tokens: 8, cache_read_input_tokens: 3,
    cache_creation_input_tokens: 6, reasoning_tokens: 4,
    model_steps: 3, covered_model_steps: 3, run_count: 2, missing_runs: 0, complete: true,
  });
  expect(final.usage).toMatchObject({
    run_input_tokens: 11, run_output_tokens: 5, run_cache_read_input_tokens: 2,
    run_cache_creation_input_tokens: 4, run_reasoning_output_tokens: 3, model_steps: 2,
  });
  expect(run.waitCalls).toBe(1);
  expect(ctx!.sdk.agents[1]!.runs[0]!.waitCalls).toBe(1);
  expect(ctx!.sdk.agents[1]!.runs[0]!.capturedToolResults).toHaveLength(2);
  const ledger = ctx!.app.ledger!;
  expect(ledger.getReceiptByRunId(oldSession.ledgerRunId!)?.usage).toEqual({
    inputTokens: 7, outputTokens: 3, cacheReadTokens: 1, cacheWriteTokens: 2, reasoningTokens: 1,
  });
  const newSession = ctx!.app.registry.get(final.cursor_session_id)!;
  expect(ledger.getReceiptByRunId(newSession.ledgerRunId!)?.usage).toEqual(finalUsage);

  const followUp = await api(ctx!, "/v1/responses", {
    method: "POST", headers: { "x-cursor-session-id": final.cursor_session_id },
    body: JSON.stringify({
      model: "composer-2.5", tools: [tools[0], plugin],
      input: [...input, ...recovered[0]!.output, {
        type: "function_call_output", call_id: recovered[0]!.output[0]!.call_id, output: "ok",
      }, ...final.output, { role: "user", content: "a new user request" }],
    }),
  });
  expect(followUp.status).toBe(200);
  const nextBody = await followUp.json() as { usage: Record<string, unknown> };
  expect(nextBody.usage.lineage_usage).toBeUndefined();
  expect(nextBody.usage).toMatchObject({ run_input_tokens: 11, model_steps: 1 });
  expect(ctx!.sdk.agents).toHaveLength(2);
  expect(ctx!.sdk.agents[1]!.sendCount).toBe(2);
});

test("native replacement with no retired SDK usage stays partial", async () => {
  const { run, input } = await openSearch();
  const wait = run.wait.bind(run);
  run.wait = async () => ({ ...await wait(), usage: undefined });
  Object.defineProperty(run, "usage", { get() { throw new Error("usage unavailable"); } });
  const recovered = await continueSearch(input);
  const final = await finish(input, recovered.output);
  expect(final.usage.lineage_usage).toEqual({
    model_steps: 3, covered_model_steps: 2, run_count: 2, missing_runs: 1, complete: false,
  });
  expect(run.waitCalls).toBe(1);
});

test.each(["credential", "model"])("does not retire or inherit a live owner's usage on a %s mismatch", async (mismatch) => {
  const { run, input } = await openSearch();
  ctx!.sdk.models = { ok: true, models: [{ id: "composer-2.5" }, { id: "other-model" }] };
  const response = await api(ctx!, "/v1/responses", {
    method: "POST", apiKey: mismatch === "credential" ? "test-key-b" : "test-key-a",
    body: JSON.stringify({ model: mismatch === "model" ? "other-model" : "composer-2.5", tools, input }),
  });
  expect(response.status).toBe(409);
  expect(run.cancelled).toBe(false);
  expect(ctx!.sdk.agents).toHaveLength(1);
  const recovered = await continueSearch(input);
  const final = await finish(input, recovered.output);
  expect(final.usage.lineage_usage).toMatchObject({ complete: true, run_count: 2, input_tokens: 18 });
});

test("a cold native transcript recovery never claims missing prior counters", async () => {
  const { oldSession, input } = await openSearch();
  ctx!.app.registry.forget(oldSession, "simulate_restart");
  ctx!.app.lineage.delete(oldSession.sessionId);
  const recovered = await continueSearch(input);
  const final = await finish(input, recovered.output);
  expect(final.usage.lineage_usage).toEqual({
    model_steps: 2, covered_model_steps: 2, run_count: 2, missing_runs: 1, complete: false,
  });
});
