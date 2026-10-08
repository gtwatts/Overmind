import type { LineageUsageView } from "../protocols/anthropic/types.js";
import type { SdkUsage } from "../sdk/port.js";

interface RunUsage {
  usage?: SdkUsage;
  modelSteps: number;
  settled: boolean;
}

const COUNTERS = {
  input_tokens: "inputTokens",
  output_tokens: "outputTokens",
  cache_read_input_tokens: "cacheReadTokens",
  cache_creation_input_tokens: "cacheWriteTokens",
  reasoning_tokens: "reasoningTokens",
} as const;

function known(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function covered(run: RunUsage): boolean {
  return run.settled && ["inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens"]
    .every((key) => known(run.usage?.[key as keyof SdkUsage]));
}

/** One logical user turn, possibly spanning replacement SDK runs. */
export class RunUsageLineage {
  private readonly runs = new Map<string, RunUsage>();
  private unknownPriorRuns = 0;
  recovered = false;

  static unknownRecovery(): RunUsageLineage {
    const lineage = new RunUsageLineage();
    lineage.recovered = true;
    // A cold transcript contains prior model work but no authoritative counters.
    lineage.unknownPriorRuns = 1;
    return lineage;
  }

  record(runId: string, usage: SdkUsage | undefined, modelSteps: number, settled: boolean): void {
    const previous = this.runs.get(runId);
    // A terminal snapshot is authoritative. Repeated cancel/getter observations
    // must not add cumulative usage again or replace it with a stale snapshot.
    if (previous?.settled) return;
    this.runs.set(runId, {
      usage: usage ? { ...usage } : settled ? undefined : previous?.usage,
      modelSteps: Math.max(previous?.modelSteps ?? 0, modelSteps),
      settled,
    });
  }

  snapshot(): LineageUsageView {
    const runs = [...this.runs.values()];
    const missingRuns = this.unknownPriorRuns + runs.filter((run) => !covered(run)).length;
    const result: LineageUsageView = {
      model_steps: runs.reduce((sum, run) => sum + run.modelSteps, 0),
      covered_model_steps: runs.filter(covered).reduce((sum, run) => sum + run.modelSteps, 0),
      run_count: runs.length + this.unknownPriorRuns,
      missing_runs: missingRuns,
      complete: runs.length > 0 && missingRuns === 0,
    };
    // Each field is present only when every contributing SDK run supplies it.
    // An optional/missing SDK cache counter is unknown, never an invented zero.
    for (const [target, source] of Object.entries(COUNTERS)) {
      if (this.unknownPriorRuns > 0 || runs.length === 0) continue;
      const values = runs.map((run) => run.usage?.[source as keyof SdkUsage]);
      if (!values.every(known)) continue;
      const total = values.reduce((sum, value) => sum + value, 0);
      if (!known(total)) {
        result.complete = false;
        continue;
      }
      result[target as keyof typeof COUNTERS] = total;
    }
    return result;
  }
}
