# Overmind Cursor helper

This directory is a vendored copy of
[cursor-sdk2api](https://github.com/Sunnyender-org/cursor-sdk2api) (MIT, see
`LICENSE` and `NOTICE.md`) at upstream commit
`b4b53b628701f1eb455678911494cde58f51f476` (v0.4.0, 2026-09-01), trimmed to the
server sources and tests (no web console, no live smoke scripts; the four
upstream tests that exercise the web console / new-api integration are dropped).

It turns the official `@cursor/sdk` into a local OpenAI Responses endpoint.
Cursor's agent harness chooses tools, but every tool is a *client* tool exposed
to Cursor through `local.customTools`; Cursor's own shell/read/edit tools are
disabled. Each Cursor tool call is parked and returned to Codex as a raw
`function_call` / `custom_tool_call`, Codex executes it with its own sandbox and
approval policy, and the result is delivered back on the next request.

The `codex-overmind-cursor` crate spawns `overmind-entry.mjs` on loopback with a
random port, BYOK auth (the Cursor key is sent per request as the bearer token
and never written by the helper), `OVERMIND_CODEX_COMPAT=1` and
`HOSTED_SEARCH_MODE=auto`. The helper exits when Overmind closes its stdin.

## Overmind patches (all marked `Overmind:` in the source)

- `src/protocols/openai-responses/parse.ts`: accept Codex's top-level
  `namespace` tools (MCP/app tools) by flattening them to qualified client tools;
  with `OVERMIND_CODEX_COMPAT=1`, drop OpenAI-hosted tools Codex always
  advertises (`image_generation`, ...) and map `web_search` onto Cursor's hosted
  search instead of failing the turn.
- `src/core/run-coordinator.ts`: when Codex's tool list changes mid-turn (an MCP
  server finishes starting), rebuild the turn from the full transcript instead of
  returning `409 session policy does not match`.
- `src/core/overmind-model-params.ts` + `src/server/app.ts`: map Codex's generic
  `reasoning.effort` onto each Cursor model's declared parameter
  (`effort` / `reasoning_effort` / `reasoning`) and nearest allowed value; drop
  undeclared parameters (Cursor rejects them as "Invalid parameters").
- `src/protocols/openai-responses/overmind-client-scope.ts` (+ small hooks in
  `parse.ts`, `src/protocols/anthropic/types.ts`, `src/core/cursor-agent-turn.ts`):
  with `OVERMIND_CODEX_COMPAT=1`, scope the ordinary-turn replay journal to
  Codex's session (`prompt_cache_key`) and turn (`client_metadata.turn_id`) so two
  Codex sessions sending an identical first prompt never share or replay each
  other's answers.
- Tests: `tests/contract/overmind-*.test.ts`.

## Build and test

```bash
npm ci --no-audit --no-fund
npm run build:server
npx vitest run
```

Overmind runs `npm ci` + `npm run build:server` automatically the first time a
Cursor model is used if `dist/` is missing.

The same patched tree runs as the stock-Codex sidecar in
`~/Documents/projects/codex-cursor-bridge` on watts.
