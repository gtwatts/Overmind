// Overmind: per-step Cursor usage for Codex's context window.
//
// One Cursor SDK run spans many model calls (one per tool round trip), but
// the SDK only reports usage once, at the end of the run, summed over every
// call (`turn-ended` fires once per run with the same total run.wait()
// returns). Codex treats each Responses usage block as the cost of one model
// call and derives the context-window % (and auto-compaction) from the
// latest one, so passing the run total through made a 9-step turn look like
// ~9x its real context (e.g. 332K "used" for a ~40K context).
//
// The tracker reconstructs the last call's context size. It records how much
// content (in estimated tokens) each step added: assistant text/thinking,
// tool-call arguments and tool results. With s_k = s_1 + D_k (D_k = content
// added before step k) and the run total C = sum(s_k):
//   s_1 = (C - sum(D_k)) / N,  s_N = s_1 + D_N.
// Mid-run tool batches report the previous run's context plus what this run
// added so far (when known); otherwise usage stays deferred as before.
import type { SdkUsage } from "../sdk/port.js";
import type { UsageView } from "../protocols/anthropic/types.js";
import { deferredUsage, fromSdkUsage } from "./usage.js";

const CHARS_PER_TOKEN = 4;

export interface StepUsageEstimate {
  view: UsageView;
  /** Estimated context occupancy (input tokens) of the run's last model call. */
  contextTokens?: number;
}

export class StepUsageTracker {
  private addedChars = 0;
  private stepOutputChars = 0;
  private readonly stepStarts: number[] = [];

  /** @param baselineTokens context size at the start of this run, if known. */
  constructor(private readonly baselineTokens?: number) {}

  /** Assistant output (text, thinking, tool-call arguments) of the current step. */
  noteOutput(chars: number): void {
    this.addedChars += chars;
    this.stepOutputChars += chars;
  }

  /** Content fed back to the model between steps (tool results). */
  noteInput(chars: number): void {
    this.addedChars += chars;
  }

  get steps(): number {
    return this.stepStarts.length;
  }

  /** Closes the current step at a tool-batch boundary. */
  forToolBatch(): UsageView {
    const outputChars = this.stepOutputChars;
    this.closeStep();
    if (this.baselineTokens === undefined) return deferredUsage();
    return {
      input_tokens: this.baselineTokens + tokens(this.addedChars - outputChars),
      output_tokens: tokens(outputChars),
      usage_status: "sdk",
    };
  }

  /** Closes the final step and converts the run total into last-call usage. */
  forFinal(runTotal: SdkUsage | undefined): StepUsageEstimate {
    this.closeStep();
    const view = fromSdkUsage(runTotal);
    const steps = this.stepStarts.length;
    if (!runTotal) return { view };
    if (steps <= 1) return { view, contextTokens: runTotal.inputTokens };
    const total = runTotal.inputTokens;
    const startSum = this.stepStarts.reduce((sum, start) => sum + start, 0);
    const first = Math.max(0, Math.min(total / steps, (total - startSum) / steps));
    const last = Math.min(total, Math.round(first + this.stepStarts[steps - 1]!));
    const scale = total > 0 ? last / total : 0;
    const estimated: UsageView = {
      ...view,
      input_tokens: last,
      output_tokens: Math.round(runTotal.outputTokens / steps),
      run_input_tokens: runTotal.inputTokens,
      run_output_tokens: runTotal.outputTokens,
      model_steps: steps,
    };
    if (typeof view.cache_read_input_tokens === "number") {
      estimated.cache_read_input_tokens = Math.round(view.cache_read_input_tokens * scale);
    }
    if (typeof view.cache_creation_input_tokens === "number") {
      estimated.cache_creation_input_tokens = Math.round(view.cache_creation_input_tokens * scale);
    }
    if (typeof view.reasoning_tokens === "number") {
      estimated.reasoning_tokens = Math.round(view.reasoning_tokens / steps);
    }
    return { view: estimated, contextTokens: last };
  }

  private closeStep(): void {
    // Content added before this step started = everything minus its own output.
    this.stepStarts.push(tokens(this.addedChars - this.stepOutputChars));
    this.stepOutputChars = 0;
  }
}

function tokens(chars: number): number {
  return Math.max(0, Math.round(chars / CHARS_PER_TOKEN));
}

export function resultChars(result: unknown): number {
  if (typeof result === "string") return result.length;
  try {
    return JSON.stringify(result ?? "").length;
  } catch {
    return 0;
  }
}
