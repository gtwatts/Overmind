import type { SdkModel } from "../sdk/port.js";

/**
 * Overmind: OpenAI-style clients (Codex) send one generic `reasoning.effort`,
 * which the parser records as the Cursor parameter `effort`. Cursor models use
 * different parameter ids (`effort`, `reasoning_effort`, `reasoning`) and value
 * sets, and Cursor rejects unknown ids/values ("Invalid parameters for registry
 * model"). Map the generic effort onto the model's real parameter and the
 * nearest allowed value, and drop parameters the model does not declare.
 */
const EFFORT_PARAM_IDS = ["effort", "reasoning_effort", "reasoning"] as const;

const EFFORT_RANK: Record<string, number> = {
  none: 0,
  minimal: 1,
  low: 2,
  medium: 3,
  high: 4,
  xhigh: 5,
  "extra-high": 5,
  max: 6,
  ultra: 6,
};

export function normalizeCursorModelParams(
  params: Array<{ id: string; value: string }>,
  model: SdkModel | undefined,
): Array<{ id: string; value: string }> {
  if (!model || params.length === 0) return params;
  const declared = new Map((model.parameters ?? []).map((p) => [p.id, p.values.map((v) => v.value)]));
  const out = new Map<string, string>();
  for (const param of params) {
    const allowed = declared.get(param.id);
    if (allowed && allowed.includes(param.value)) {
      out.set(param.id, param.value);
      continue;
    }
    if (!(EFFORT_PARAM_IDS as readonly string[]).includes(param.id)) continue;
    const target = EFFORT_PARAM_IDS.find((id) => declared.has(id));
    if (!target) continue;
    const mapped = nearestEffort(param.value, declared.get(target) ?? []);
    if (mapped !== undefined && !out.has(target)) out.set(target, mapped);
  }
  return [...out.entries()]
    .map(([id, value]) => ({ id, value }))
    .sort((a, b) => a.id.localeCompare(b.id));
}

function nearestEffort(requested: string, allowed: string[]): string | undefined {
  const exact = allowed.find((v) => v === requested);
  if (exact) return exact;
  const want = EFFORT_RANK[requested.toLowerCase()];
  if (want === undefined) return undefined;
  // "none" means "no explicit effort": let Cursor use the model default
  // unless the model actually offers a "none" level.
  if (want === 0) return undefined;
  let best: string | undefined;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const value of allowed) {
    const rank = EFFORT_RANK[value.toLowerCase()];
    if (rank === undefined) continue;
    const distance = Math.abs(rank - want);
    // ties resolve toward the lower (cheaper) level
    if (distance < bestDistance || (distance === bestDistance && best !== undefined && rank < (EFFORT_RANK[best] ?? 0))) {
      best = value;
      bestDistance = distance;
    }
  }
  return best;
}
