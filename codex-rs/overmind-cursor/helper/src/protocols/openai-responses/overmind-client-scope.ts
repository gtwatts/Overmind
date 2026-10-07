// Overmind patch: scope ordinary-turn replay/lineage keys to the Codex client
// session (prompt_cache_key) and turn (client_metadata.turn_id). Without this,
// two separate Codex sessions that send byte-identical first turns collide in
// the replay journal: the second one is either served the first one's cached
// answer or fails with "cannot be replayed" after a helper restart.
// Enabled only when OVERMIND_CODEX_COMPAT=1 so upstream behaviour is unchanged.

export interface ClientScope {
  session?: string;
  turn?: string;
}

const MAX_SCOPE_LEN = 200;

function scopeValue(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > MAX_SCOPE_LEN) return undefined;
  return trimmed;
}

export function overmindClientScope(raw: Record<string, unknown>): ClientScope | undefined {
  if (process.env.OVERMIND_CODEX_COMPAT !== "1") return undefined;
  const session = scopeValue(raw.prompt_cache_key);
  const metadata = raw.client_metadata;
  const turn =
    metadata && typeof metadata === "object" && !Array.isArray(metadata)
      ? scopeValue((metadata as Record<string, unknown>).turn_id)
      : undefined;
  if (!session && !turn) return undefined;
  return { ...(session ? { session } : {}), ...(turn ? { turn } : {}) };
}
