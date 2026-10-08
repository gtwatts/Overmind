import { describe, expect, it } from "vitest";
import { resultChars, StepUsageTracker } from "../../src/core/step-usage.js";
import { encodeResponsesUsage } from "../../src/protocols/openai-responses/encode.js";

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

    // SDK input and caches are additive: total context is 105.6K across three
    // calls. With the observed additions, their contexts are 33.2K,35.2K,37.2K.
    expect(tracker.forFinal({
      inputTokens: 66_000,
      outputTokens: 600,
      cacheReadTokens: 33_000,
      cacheWriteTokens: 6_600,
      reasoningTokens: 300,
    })).toEqual({
      contextTokens: 37_200,
      view: {
        input_tokens: 23_250,
        output_tokens: 200,
        cache_read_input_tokens: 11_625,
        cache_creation_input_tokens: 2_325,
        reasoning_tokens: 100,
        usage_status: "sdk",
        run_input_tokens: 66_000,
        run_output_tokens: 600,
        run_cache_read_input_tokens: 33_000,
        run_cache_creation_input_tokens: 6_600,
        run_reasoning_output_tokens: 300,
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
      contextTokens: 45_100,
      view: {
        input_tokens: 25_000,
        output_tokens: 10,
        cache_read_input_tokens: 20_000,
        cache_creation_input_tokens: 100,
        reasoning_tokens: 5,
        usage_status: "sdk",
        run_input_tokens: 25_000,
        run_output_tokens: 10,
        run_cache_read_input_tokens: 20_000,
        run_cache_creation_input_tokens: 100,
        run_reasoning_output_tokens: 5,
        model_steps: 1,
      },
    });
  });

  it("exports exact run cache and reasoning totals separately from estimated context usage", () => {
    const tracker = new StepUsageTracker();
    tracker.noteOutput(400);
    tracker.forToolBatch();
    tracker.noteInput(3_600);
    const { view } = tracker.forFinal({
      inputTokens: 41_000,
      outputTokens: 500,
      cacheReadTokens: 30_000,
      cacheWriteTokens: 0,
      reasoningTokens: 101,
    });
    const usage = encodeResponsesUsage({
      messageId: "msg_usage",
      sessionId: "session_usage",
      model: "composer-2.5",
      stopReason: "end_turn",
      blocks: [],
      usage: view,
    });
    expect(usage).toMatchObject({
      input_tokens: 36_000,
      output_tokens: 250,
      run_input_tokens: 41_000,
      run_output_tokens: 500,
      run_cache_read_input_tokens: 30_000,
      run_cache_creation_input_tokens: 0,
      run_reasoning_output_tokens: 101,
      model_steps: 2,
    });
    expect(usage.input_tokens_details.cached_tokens).toBe(15_211);
    expect(usage.output_tokens_details.reasoning_tokens).toBe(51);
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

  it("includes additive SDK caches in single-call Responses input without double-counting reasoning", () => {
    const tracker = new StepUsageTracker();
    const { view, contextTokens } = tracker.forFinal({
      inputTokens: 25_000,
      outputTokens: 10,
      cacheReadTokens: 20_000,
      cacheWriteTokens: 100,
      reasoningTokens: 5,
    });
    expect(contextTokens).toBe(45_100);
    expect(encodeResponsesUsage({
      messageId: "msg_usage",
      sessionId: "session_usage",
      model: "composer-2.5",
      stopReason: "end_turn",
      blocks: [],
      usage: view,
    })).toMatchObject({
      input_tokens: 45_100,
      output_tokens: 10,
      total_tokens: 45_110,
      input_tokens_details: { cached_tokens: 20_000 },
      output_tokens_details: { reasoning_tokens: 5 },
      run_input_tokens: 25_000,
      run_cache_read_input_tokens: 20_000,
      run_cache_creation_input_tokens: 100,
      model_steps: 1,
    });
  });

  it("bounds rounded cache components by the reconstructed context", () => {
    const tracker = new StepUsageTracker();
    tracker.forToolBatch();
    const { view, contextTokens } = tracker.forFinal({
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 1,
      cacheWriteTokens: 1,
    });
    expect(contextTokens).toBe(1);
    expect(view.input_tokens).toBe(0);
    expect(view.cache_read_input_tokens).toBe(1);
    expect(view.cache_creation_input_tokens).toBe(0);
    expect(view.run_cache_creation_input_tokens).toBe(1);
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
