import { describe, expect, it } from "vitest";
import { resultChars, StepUsageTracker } from "../../src/core/step-usage.js";

describe("Overmind per-step Cursor usage", () => {
  it("defers tool batches without a previous context estimate", () => {
    const tracker = new StepUsageTracker();
    tracker.noteOutput(400);
    expect(tracker.forToolBatch()).toEqual({
      input_tokens: 0,
      output_tokens: 0,
      usage_deferred: true,
      usage_status: "deferred",
    });
    expect(tracker.steps).toBe(1);
  });

  it("keeps the current step's output out of its input context", () => {
    const tracker = new StepUsageTracker(20_000);
    tracker.noteOutput(400);
    expect(tracker.forToolBatch()).toEqual({
      input_tokens: 20_000,
      output_tokens: 100,
      usage_status: "sdk",
    });

    // The previous assistant output and tool results enter the next call's context.
    tracker.noteInput(7_600);
    tracker.noteOutput(200);
    expect(tracker.forToolBatch()).toEqual({
      input_tokens: 22_000,
      output_tokens: 50,
      usage_status: "sdk",
    });
    expect(tracker.steps).toBe(2);
  });

  it("reconstructs the last call's context and preserves the SDK run totals", () => {
    const tracker = new StepUsageTracker();
    tracker.noteOutput(400);
    tracker.forToolBatch();
    tracker.noteInput(7_600);
    tracker.noteOutput(200);
    tracker.forToolBatch();
    tracker.noteInput(7_800);
    tracker.noteOutput(1_800);

    // Model inputs were 20K, 22K and 24K; the SDK reports their 66K sum.
    expect(tracker.forFinal({
      inputTokens: 66_000,
      outputTokens: 600,
      cacheReadTokens: 33_000,
      cacheWriteTokens: 6_600,
      reasoningTokens: 300,
    })).toEqual({
      contextTokens: 24_000,
      view: {
        input_tokens: 24_000,
        output_tokens: 200,
        cache_read_input_tokens: 12_000,
        cache_creation_input_tokens: 2_400,
        reasoning_tokens: 100,
        usage_status: "sdk",
        run_input_tokens: 66_000,
        run_output_tokens: 600,
        model_steps: 3,
      },
    });
  });

  it("counts a batch of parallel tools as one model step", () => {
    const tracker = new StepUsageTracker();
    tracker.noteOutput(100);
    tracker.noteOutput(300);
    tracker.forToolBatch();
    tracker.noteInput(4_000);
    tracker.noteInput(3_600);
    tracker.noteOutput(400);
    expect(tracker.forFinal({ inputTokens: 42_000, outputTokens: 200 })).toEqual({
      contextTokens: 22_000,
      view: {
        input_tokens: 22_000,
        output_tokens: 100,
        usage_status: "sdk",
        run_input_tokens: 42_000,
        run_output_tokens: 200,
        model_steps: 2,
      },
    });
  });

  it("preserves exact SDK usage for a single-call run", () => {
    const tracker = new StepUsageTracker(10_000);
    tracker.noteOutput(40);
    expect(tracker.forFinal({
      inputTokens: 25_000,
      outputTokens: 10,
      cacheReadTokens: 20_000,
      cacheWriteTokens: 100,
      reasoningTokens: 5,
    })).toEqual({
      contextTokens: 25_000,
      view: {
        input_tokens: 25_000,
        output_tokens: 10,
        cache_read_input_tokens: 20_000,
        cache_creation_input_tokens: 100,
        reasoning_tokens: 5,
        usage_status: "sdk",
      },
    });
  });

  it("does not invent final usage when the SDK reports none", () => {
    const tracker = new StepUsageTracker(20_000);
    tracker.noteOutput(400);
    tracker.forToolBatch();
    tracker.noteInput(7_600);
    expect(tracker.forFinal(undefined)).toEqual({
      view: { input_tokens: 0, output_tokens: 0, usage_status: "unavailable" },
    });
  });

  it.each([0, 40])("bounds context estimates by the reported total of %i tokens", (total) => {
    const tracker = new StepUsageTracker();
    tracker.noteOutput(400);
    tracker.forToolBatch();
    tracker.noteInput(16_000);
    const estimate = tracker.forFinal({ inputTokens: total, outputTokens: 10, cacheReadTokens: 0 });
    expect(estimate.contextTokens).toBe(total);
    expect(estimate.view.input_tokens).toBe(total);
    expect(estimate.view.cache_read_input_tokens).toBe(0);
  });
});

describe("Overmind tool content sizing", () => {
  it("counts strings directly and structured tool results as JSON", () => {
    expect(resultChars("done")).toBe(4);
    expect(resultChars({ content: [{ type: "text", text: "done" }] })).toBe(
      JSON.stringify({ content: [{ type: "text", text: "done" }] }).length,
    );
  });

  it("does not fail the turn on an unserializable tool result", () => {
    const result: { self?: unknown } = {};
    result.self = result;
    expect(resultChars(result)).toBe(0);
  });
});
