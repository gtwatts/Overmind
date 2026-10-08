import { describe, expect, it } from "vitest";
import { StepUsageTracker } from "../../src/core/step-usage.js";

const step = (inputTokens: number, outputTokens: number) => ({
  inputTokens,
  outputTokens,
  cacheReadTokens: inputTokens - 1000,
  cacheWriteTokens: 0,
});

describe("Overmind per-step Cursor usage", () => {
  it("defers tool batches until the SDK reports a step", () => {
    const tracker = new StepUsageTracker();
    expect(tracker.forToolBatch().usage_status).toBe("deferred");
  });

  it("reports the latest step's context, not the run total", () => {
    const tracker = new StepUsageTracker();
    tracker.record(step(20_000, 100));
    expect(tracker.forToolBatch()).toMatchObject({ input_tokens: 20_000, output_tokens: 100, usage_status: "sdk" });
    tracker.record(step(22_000, 50));
    tracker.record(step(24_000, 70));
    expect(tracker.forToolBatch()).toMatchObject({
      input_tokens: 24_000,
      output_tokens: 120,
      cache_read_input_tokens: 23_000,
    });
    tracker.record(step(26_000, 300));
    expect(tracker.forFinal({ inputTokens: 92_000, outputTokens: 520 })).toMatchObject({
      input_tokens: 26_000,
      output_tokens: 300,
    });
  });

  it("does not double count output when every step was already reported", () => {
    const tracker = new StepUsageTracker();
    tracker.record(step(20_000, 100));
    tracker.forToolBatch();
    expect(tracker.forFinal({ inputTokens: 20_000, outputTokens: 100 })).toMatchObject({
      input_tokens: 20_000,
      output_tokens: 0,
    });
  });

  it("falls back to the run total without per-step data", () => {
    const tracker = new StepUsageTracker();
    expect(tracker.forFinal({ inputTokens: 25_000, outputTokens: 10 })).toMatchObject({
      input_tokens: 25_000,
      output_tokens: 10,
      usage_status: "sdk",
    });
    expect(tracker.forFinal(undefined).usage_status).toBe("unavailable");
  });
});
