import { afterEach, describe, expect, it } from "vitest";
import { cursorAgentTurnFromParsed, ordinaryReplayKey } from "../../src/core/cursor-agent-turn.js";
import { parseResponsesRequest } from "../../src/protocols/openai-responses/parse.js";

const tenant = "a".repeat(64);

function request(session: string, turn: string) {
  return {
    model: "composer-2.5",
    input: [{ role: "user", content: [{ type: "input_text", text: "count files" }] }],
    stream: true,
    store: false,
    prompt_cache_key: session,
    client_metadata: { turn_id: turn },
  };
}

function turnFor(body: Record<string, unknown>) {
  const { parsed } = parseResponsesRequest(body, { hostedSearchMode: "off" });
  return cursorAgentTurnFromParsed(parsed, { tenantScope: tenant });
}

afterEach(() => {
  delete process.env.OVERMIND_CODEX_COMPAT;
});

describe("Overmind client scope", () => {
  it("keeps upstream keys when compat mode is off", () => {
    const one = turnFor(request("s1", "t1"));
    const two = turnFor(request("s2", "t2"));
    expect(one.tenantScope).toBe(tenant);
    expect(ordinaryReplayKey(one)).toBe(ordinaryReplayKey(two));
  });

  it("separates identical requests from different Codex sessions and turns", () => {
    process.env.OVERMIND_CODEX_COMPAT = "1";
    const one = turnFor(request("s1", "t1"));
    const retry = turnFor(request("s1", "t1"));
    const otherTurn = turnFor(request("s1", "t2"));
    const otherSession = turnFor(request("s2", "t1"));
    expect(one.tenantScope).toMatch(/^[a-f0-9]{64}$/);
    expect(one.tenantScope).not.toBe(tenant);
    expect(ordinaryReplayKey(retry)).toBe(ordinaryReplayKey(one));
    expect(ordinaryReplayKey(otherTurn)).not.toBe(ordinaryReplayKey(one));
    expect(otherTurn.tenantScope).toBe(one.tenantScope);
    expect(ordinaryReplayKey(otherSession)).not.toBe(ordinaryReplayKey(one));
    expect(otherSession.tenantScope).not.toBe(one.tenantScope);
  });
});
