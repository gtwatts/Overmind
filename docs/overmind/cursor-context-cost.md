# Why a trivial Cursor turn costs ~165K input tokens

Investigated on 2026-10-07 (branch `overmind/m2-pipelines`). Numbers come from Todd's real
session logs (read only), from request bodies captured with a local fake endpoint (no model
call), and from **one** live `composer-2.5` turn used for an A/B check.

## Summary

| Measurement | Input tokens |
| --- | --- |
| OpenAI (`gpt-6-astra` / `gpt-6.1-sol`), first turn, same setup | 16K to 25K |
| Cursor `composer-2.5`, trivial turn, Todd's normal setup | 165,212 (cached 16,512) |
| Cursor `grok-4.7` / `grok-4.6`, trivial turn | 171,518 / 179,579 |
| Cursor `composer-2.5`, trivial turn, **plugins and apps disabled** (live A/B) | **25,043** |
| Cursor turns with 2 to 4 tool calls | 485K / 728K (more than the 180K/230K window) |

1. **Plugin MCP tool schemas are about 85% of the cost.** OpenAI models run in
   `tool_mode = "code_mode_only"` with `supports_search_tool = true`, so Codex sends MCP tools
   in deferred form and the request stays around 90K characters. The Cursor catalog
   (`overmind-cursor/src/cursor_models.json`) has neither setting. Codex therefore sends every
   MCP tool from every enabled plugin as a full JSON schema on every request: 25 tool entries
   and 174K characters of tool JSON, against 45K characters for the whole prompt. The helper
   passes them to Cursor as `local.customTools`. Cursor renders those schemas expensively:
   about 166K extra characters of tool JSON become about 140K extra input tokens.
   Turning plugins off cut the same trivial turn from 165K to 25K tokens.

   The biggest namespaces, in characters of schema JSON:

   | Namespace | Tools | Characters |
   | --- | --- | --- |
   | `ffmpeg_skill` | 21 | 52,193 |
   | `blender` | 28 | 25,815 |
   | `gordon_tools` | 27 | 24,373 |
   | `agent_board` | 22 | 13,127 |
   | `photocraft` | 20 | 11,523 |
   | `multi_agent_v1` | 5 | 9,933 |

   The rest (`herdr`, `cursor_bridge`, `slopmonster`, `taxscout`, `clefgrep`,
   `obsidian_memory`, `node_repl`, plus the built-ins) total about 40K.
2. **Usage is reported cumulatively for a whole Cursor run.** A Cursor run spans every tool
   round trip of a turn. The helper waits on `run.wait()` and puts the *sum over all model steps*
   into the final response. Intermediate responses report 0 tokens (`usage_deferred`). Codex
   reads a response's `input_tokens` as current context occupancy, so the context gauge shows
   200% to 300% after any turn with tool calls (485K on a 180K window). Auto-compaction keys off
   the same number, so long Cursor sessions will compact far too early. The billing total is
   right; the per-request context figure is not.
3. **Not a significant cause:** the skills list (about 19K to 22K characters, roughly 5K tokens),
   AGENTS.md (3K characters) and Codex base instructions. The helper starts Cursor with
   `settingSources: []`, an empty workspace and a built-in tool allowlist, so Cursor rules and
   skills are not loaded on top. Cursor's own harness is about 16K tokens (the cached part of the
   trivial turn).

## The "119 skills didn't fit in the skills context budget" warning

Codex caps the skills list at 2% of the context window (`SKILL_METADATA_CONTEXT_WINDOW_PERCENT`).
For `composer-2.5` that is 2% of 180K, about 3.6K tokens. Todd has about 250 skills, mostly from
plugins. Codex lists about 130 with shortened descriptions and leaves out the rest, which triggers
the warning. With OpenAI the budget is 2% of about 258K, so fewer skills are dropped.

This is an availability problem, not a token problem: the 119 left-out skills can't be found
by name in that session. Raising the budget would add tokens. The real fix is fewer always-on
plugins (see below), which also shrinks the tool list.

## Proposed fixes

| # | Fix | Effect | Risk | Where | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | Trim which plugins are enabled for Cursor sessions. Use a Cursor profile such as `composer.config.toml` / `grok.config.toml` with heavy plugins disabled, or `enabled_tools` per MCP server. `ffmpeg_skill`, `blender`, `gordon_tools` and `agent_board` alone are 115K characters | Measured 165K → 25K with all plugins off; partial trims scale with the characters removed | none (config) | `~/.codex` profiles | **needs Todd**: which plugins Cursor sessions need |
| 2 | Support Codex's client-side `tool_search` in the helper, then set `supports_search_tool: true` for Cursor models. Deferred MCP tools would be loaded on demand, the way OpenAI models get them | Expected to bring default Cursor turns near the 25K baseline with every plugin still available | medium (helper protocol work, needs live tests) | `overmind-cursor/helper` + catalog | proposed |
| 3 | Report per-step usage. Attach each Cursor step's `turn-ended` usage to the Codex response it produced, and use the remainder for the final response, so the context gauge and auto-compaction see real occupancy | Correct context %; no premature compaction | low to medium (needs SDK event-order check, live test) | `overmind-cursor/helper/src/core/event-pump.ts` | proposed |
| 4 | Make the 2% skills budget configurable per provider, or let `skill_search` cover left-out skills for Cursor | Silences the warning without adding tokens | low | `ext/skills` | proposed; prefer fix 1 first |

None of the token fixes were implemented on this branch. Fix 1 is a change to Todd's
configuration under `~/.codex`, which this work must not touch. Fixes 2 and 3 change the
helper's protocol handling and need live turns to verify, so they are not low risk.

## How to reproduce without spending tokens

```bash
# Private copy of the inputs (no auth.json, no secrets), then capture the request body:
python3 capture.py cursor 18777 &   # tiny HTTP server that saves the POST body and returns 400
CODEX_HOME=/tmp/om-ctx-home CURSOR_API_KEY=dummy \
OVERMIND_CURSOR_BASE_URL=http://127.0.0.1:18777/v1 \
  target/debug/codex exec -m composer-2.5 --skip-git-repo-check "hi" </dev/null
# add -c features.plugins=false -c features.apps=false to see the plugin-free request
```
