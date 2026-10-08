// Overmind: per-step Cursor usage.
//
// One Cursor SDK run spans many model calls (one per tool round trip), and
// the usage returned by run.wait() is the sum over all of them. Codex treats
// each Responses usage block as the cost of one model call and derives the
// context-window % (and auto-compaction) from the latest one, so reporting
// the run total on the final response made a 6-step turn look like 6x the
// real context. The SDK emits `turn-ended` updates carrying each step's
// usage; this tracker attributes them to the response segment that produced
// them.
import type { SdkUsage } from "../sdk/port.js";
import type { UsageView } from "../protocols/anthropic/types.js";
import { deferredUsage, fromSdkUsage } from "./usage.js";

export class StepUsageTracker {
  private readonly steps: SdkUsage[] = [];
  private reported = 0;

  record(usage: SdkUsage | undefined): void {
    if (!usage) return;
    this.steps.push(usage);
    if (process.env.OVERMIND_USAGE_DEBUG === "1") {
      console.log(JSON.stringify({ msg: "overmind step usage", step: this.steps.length, usage }));
    }
  }

  get stepCount(): number {
    return this.steps.length;
  }

  /**
   * Usage for a tool-batch boundary: the latest unreported step's context
   * (input) size plus every unreported step's output. Deferred when the SDK
   * has not reported a step yet.
   */
  forToolBatch(): UsageView {
    return this.takeUnreported() ?? deferredUsage();
  }

  /**
   * Usage for the final boundary. With per-step data, report the remaining
   * steps the same way as tool batches. Without it, fall back to the run
   * total (correct for single-step runs, which is the common no-tool case).
   */
  forFinal(runTotal: SdkUsage | undefined): UsageView {
    const unreported = this.takeUnreported();
    if (unreported) return unreported;
    if (this.steps.length > 0) {
      // Everything was already attributed to earlier tool batches; the final
      // text came from the last reported step, so repeat its context size
      // without double counting output.
      const last = this.steps[this.steps.length - 1]!;
      return fromSdkUsage({ ...last, outputTokens: 0, reasoningTokens: 0 });
    }
    return fromSdkUsage(runTotal);
  }

  private takeUnreported(): UsageView | undefined {
    if (this.reported >= this.steps.length) return undefined;
    const pending = this.steps.slice(this.reported);
    this.reported = this.steps.length;
    const last = pending[pending.length - 1]!;
    const merged: SdkUsage = {
      ...last,
      outputTokens: pending.reduce((sum, step) => sum + step.outputTokens, 0),
    };
    const reasoning = pending.reduce((sum, step) => sum + (step.reasoningTokens ?? 0), 0);
    if (reasoning > 0 || typeof last.reasoningTokens === "number") merged.reasoningTokens = reasoning;
    return fromSdkUsage(merged);
  }
}
