import { describe, expect, it, vi } from "vitest";
import { FakeClock } from "../../src/clock.js";
import { EventPump } from "../../src/core/event-pump.js";
import { SdkRunDriver } from "../../src/core/sdk-run-driver.js";
import { Session } from "../../src/core/session.js";
import { SessionRegistry } from "../../src/core/session-registry.js";
import { completedToolSignature } from "../../src/core/tool-bridge.js";
import { RunUsageLineage } from "../../src/core/usage-lineage.js";
import type { AnthropicTool } from "../../src/protocols/anthropic/types.js";
import { encodeUsage } from "../../src/protocols/anthropic/encode.js";
import { encodeResponsesUsage } from "../../src/protocols/openai-responses/encode.js";
import type { SdkRun, SdkUsage } from "../../src/sdk/port.js";
import { FakeSdk } from "../fixtures/fake-sdk.js";

const retired: SdkUsage = {
  inputTokens: 7, outputTokens: 3, cacheReadTokens: 1, cacheWriteTokens: 2, reasoningTokens: 1,
};
const current: SdkUsage = {
  inputTokens: 11, outputTokens: 5, cacheReadTokens: 2, cacheWriteTokens: 4, reasoningTokens: 3,
};
const owner = {
  credentialFingerprint: "tenant-a", modelId: "composer-2.5", sessionPolicyFingerprint: "policy",
  executableToolCatalogFingerprint: "catalog", instanceId: "instance",
};
const search: AnthropicTool = {
  name: "tool_search", tool_kind: "tool_search", input_schema: { type: "object" },
};
const plugin: AnthropicTool = {
  name: "dispatch", sdk_name: "mcp__fixture__dispatch", namespace: "mcp__fixture",
  input_schema: { type: "object" },
};

describe("raw usage lineage coverage", () => {
  it("replaces cumulative snapshots per SDK run and ignores repeated terminal/late getter snapshots", () => {
    const lineage = new RunUsageLineage();
    lineage.record("a", { ...retired, inputTokens: 1 }, 1, false);
    lineage.record("a", { ...retired, inputTokens: 4 }, 1, false);
    lineage.record("a", retired, 1, true);
    lineage.record("a", { ...retired, inputTokens: 99 }, 1, true);
    lineage.record("a", undefined, 1, false);
    lineage.record("b", current, 2, true);
    expect(lineage.snapshot()).toEqual({
      input_tokens: 18, output_tokens: 8, cache_read_input_tokens: 3,
      cache_creation_input_tokens: 6, reasoning_tokens: 4,
      model_steps: 3, covered_model_steps: 3, run_count: 2, missing_runs: 0, complete: true,
    });
  });

  it.each([undefined, { inputTokens: 7, outputTokens: 3 }, { ...retired, cacheReadTokens: NaN }])(
    "keeps missing or invalid terminal counters explicitly partial (%j)", (usage) => {
      const lineage = new RunUsageLineage();
      lineage.record("a", usage, 1, true);
      lineage.record("b", current, 2, true);
      expect(lineage.snapshot()).toMatchObject({
        model_steps: 3, covered_model_steps: 2, run_count: 2, missing_runs: 1, complete: false,
      });
      expect(lineage.snapshot().cache_read_input_tokens).toBeUndefined();
      if (!usage) expect(lineage.snapshot().input_tokens).toBeUndefined();
    },
  );

  it("does not require optional reasoning counters and does not invent them", () => {
    const lineage = new RunUsageLineage();
    lineage.record("a", { ...retired, reasoningTokens: undefined }, 1, true);
    expect(lineage.snapshot()).toMatchObject({ complete: true, missing_runs: 0 });
    expect(lineage.snapshot().reasoning_tokens).toBeUndefined();
  });

  it("marks cold transcript recovery as unknown even when the new run reports exact usage", () => {
    const lineage = RunUsageLineage.unknownRecovery();
    lineage.record("new", current, 2, true);
    expect(lineage.snapshot()).toEqual({
      model_steps: 2, covered_model_steps: 2, run_count: 2, missing_runs: 1, complete: false,
    });
  });

  it("keeps summed counter overflow unknown", () => {
    const lineage = new RunUsageLineage();
    lineage.record("a", { ...retired, inputTokens: Number.MAX_SAFE_INTEGER }, 1, true);
    lineage.record("b", current, 2, true);
    expect(lineage.snapshot().complete).toBe(false);
    expect(lineage.snapshot().input_tokens).toBeUndefined();
  });

  it("publishes raw lineage components without changing the Responses context estimate", () => {
    const lineage = new RunUsageLineage();
    lineage.record("a", retired, 1, true);
    lineage.record("b", current, 2, true);
    const turn = {
      messageId: "message", sessionId: "session", model: "composer-2.5", stopReason: "end_turn" as const,
      blocks: [], usage: {
        input_tokens: 8, output_tokens: 3, cache_read_input_tokens: 1, cache_creation_input_tokens: 3,
        run_input_tokens: 11, run_output_tokens: 5, model_steps: 2, lineage_usage: lineage.snapshot(),
      },
    };
    expect(encodeResponsesUsage(turn)).toMatchObject({
      input_tokens: 12, output_tokens: 3, total_tokens: 15,
      run_input_tokens: 11, run_output_tokens: 5, model_steps: 2,
      lineage_usage: { input_tokens: 18, cache_read_input_tokens: 3, cache_creation_input_tokens: 6 },
    });
    expect(encodeUsage(turn)).toEqual({
      input_tokens: 8, output_tokens: 3, cache_read_input_tokens: 1, cache_creation_input_tokens: 3,
      lineage_usage: lineage.snapshot(),
    });
  });
});

it("drains a cancelled parked pump once, carries usage across catalog replacement, and resets on the next user send", async () => {
  const clock = new FakeClock();
  const registry = new SessionRegistry(clock, "instance", {
    globalActiveRuns: 4, perCredentialActiveRuns: 4, maxAwaitingSessions: 4,
    sessionTtlMs: 10_000, replayTtlMs: 10_000, runDeadlineMs: 60_000,
  });
  const sdk = new FakeSdk({ finalUsage: current, agentScripts: [
    [[{ type: "tools", calls: [{ name: "tool_search", input: { query: "dispatch" } }] }]],
    [[
      { type: "tools", calls: [{ name: "tool_search", input: { query: "dispatch" } }] },
      { type: "tools", calls: [{ name: "mcp__fixture__dispatch", input: { parcel: "p1" } }] },
      { type: "text", chunks: ["dispatched"] },
    ], [{ type: "text", chunks: ["next user turn"] }]],
  ] });
  const driver = new SdkRunDriver({ sdk, clock, toolBatchSettleMs: 0, firstEventTimeoutMs: 10_000 });
  const source = { type: "create" as const, apiKey: "test-key", workspaceDir: "/tmp" };
  const oldSession = registry.create(owner);
  const oldPump = await driver.start({ session: oldSession, tools: [search], agent: source, send: { text: "dispatch" } });
  oldPump.start();
  expect((await oldPump.waitForBoundary()).type).toBe("tools");
  const oldRun = sdk.agents[0]!.runs[0]!;
  const wait = oldRun.wait.bind(oldRun);
  oldRun.wait = async () => {
    const result = await wait();
    await new Promise((resolve) => setTimeout(resolve, 20));
    return { ...result, usage: retired };
  };
  const cancel = vi.spyOn(oldRun, "cancel");
  const close = vi.spyOn(sdk.agents[0]!, "close");
  const lineage = oldSession.usageLineage;
  lineage.recovered = true;
  await Promise.all([
    registry.retireForReplacement(oldSession, "tool_catalog_changed"),
    registry.retireForReplacement(oldSession, "tool_catalog_changed"),
  ]);
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(close).toHaveBeenCalledTimes(1);
  expect(oldRun.waitCalls).toBe(1);
  expect(sdk.agents[0]!.closed).toBe(true);
  expect(oldPump.settledRunUsage()).toMatchObject({ input_tokens: 7, output_tokens: 3 });

  const next = registry.create(owner);
  const pump = await driver.start({
    session: next, tools: [search, plugin], agent: source, send: { text: "recovered transcript" }, usageLineage: lineage,
    completedResults: new Map([[completedToolSignature("tool_search", { query: "dispatch" }), ["loaded"]]]),
  });
  pump.start();
  expect((await pump.waitForBoundary()).type).toBe("tools");
  const call = [...next.pending.values()][0]!;
  expect(call.name).toBe("dispatch");
  pump.beginNextSegment();
  call.resolved = true;
  call.resolve("ok");
  const final = await pump.waitForBoundary();
  expect(final.type).toBe("final");
  if (final.type !== "final") throw new Error("missing final boundary");
  expect(final.turn.usage).toMatchObject({ run_input_tokens: 11, run_output_tokens: 5, model_steps: 2 });
  expect(final.turn.usage.lineage_usage).toEqual({
    input_tokens: 18, output_tokens: 8, cache_read_input_tokens: 3,
    cache_creation_input_tokens: 6, reasoning_tokens: 4,
    model_steps: 3, covered_model_steps: 3, run_count: 2, missing_runs: 0, complete: true,
  });
  expect(final.runUsage).toMatchObject({ input_tokens: 11, output_tokens: 5 });
  expect(sdk.agents[1]!.runs[0]!.waitCalls).toBe(1);
  const nextPump = await driver.start({
    session: next, tools: [search, plugin], agent: { type: "existing", agent: next.agent! }, send: { text: "new user turn" },
  });
  nextPump.start();
  const followUp = await nextPump.waitForBoundary();
  expect(followUp.type).toBe("final");
  if (followUp.type !== "final") throw new Error("missing follow-up boundary");
  expect(followUp.turn.usage.lineage_usage).toBeUndefined();
  expect(next.usageLineage.snapshot()).toMatchObject({ run_count: 1, model_steps: 1, input_tokens: 11 });
  expect(lineage.snapshot()).toMatchObject({ run_count: 2, model_steps: 3, input_tokens: 18 });
});

it("leaves usage partial when the existing SDK wait and usage getter both throw", async () => {
  const clock = new FakeClock();
  const session = new Session({ ...owner, clock });
  const wait = vi.fn(async () => { throw new Error("SDK wait failed"); });
  const run: SdkRun = {
    id: "failed", cancel: vi.fn(async () => undefined), wait,
    async *stream() {},
    get usage(): SdkUsage { throw new Error("SDK usage failed"); },
  };
  const pump = new EventPump(session, run, clock, 0, 10_000);
  const pending = session.createPending("tool_search", {}, clock);
  void pending.promise.catch(() => undefined);
  pump.notifyTool(pending);
  pump.start();
  expect((await pump.waitForBoundary()).type).toBe("tools");
  await pump.retireForReplacement();
  expect(wait).toHaveBeenCalledTimes(1);
  expect(pump.settledRunUsage()).toBeUndefined();
  expect(session.usageLineage.snapshot()).toEqual({
    model_steps: 1, covered_model_steps: 0, run_count: 1, missing_runs: 1, complete: false,
  });
});

it.each(["throws", "hangs"])("bounds retirement when cancellation %s and the usage getter throws", async (failure) => {
  const clock = new FakeClock();
  const session = new Session({ ...owner, clock });
  let release!: () => void;
  const done = new Promise<void>((resolve) => { release = resolve; });
  const cancel = vi.fn(async () => {
    if (failure === "throws") throw new Error("cancel failed");
    await done;
  });
  const wait = vi.fn(async () => ({ id: "blocked", status: "cancelled" as const, usage: retired }));
  const run: SdkRun = {
    id: "blocked", cancel, wait,
    async *stream() { await done; },
    get usage(): SdkUsage { throw new Error("usage unavailable"); },
  };
  const pump = new EventPump(session, run, clock, 0, 10_000);
  const pending = session.createPending("tool_search", {}, clock);
  void pending.promise.catch(() => undefined);
  pump.notifyTool(pending);
  pump.start();
  expect((await pump.waitForBoundary()).type).toBe("tools");
  let finished = false;
  const retirement = pump.retireForReplacement().then(() => { finished = true; });
  await Promise.resolve();
  clock.advance(499);
  await Promise.resolve();
  expect(finished).toBe(false);
  clock.advance(1);
  await retirement;
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(wait).not.toHaveBeenCalled();
  expect(session.usageLineage.snapshot()).toEqual({
    model_steps: 1, covered_model_steps: 0, run_count: 1, missing_runs: 1, complete: false,
  });
  // Cancellation/drain continues asynchronously after the request's deadline.
  release();
  expect((await pump.waitForBoundary()).type).toBe("tools");
  await vi.waitFor(() => expect(wait).toHaveBeenCalledTimes(1));
  expect(session.usageLineage.snapshot()).toMatchObject({ complete: true, input_tokens: 7, model_steps: 1 });
});
