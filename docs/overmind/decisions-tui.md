# Decisions tool selection and pipeline progress

Overmind can use the OpenAI Decisions API to choose an initial set of plugin MCP
tool definitions for each user turn. OpenAI and Cursor use the same selector.
Selection changes which definitions the model sees; it does not grant permission
to execute a tool or remove the remaining tools from lazy discovery.
The request format follows the official [Decisions guide](https://developers.openai.com/api/docs/guides/decisions)
and [create reference](https://developers.openai.com/api/reference/resources/decisions/methods/create).

## Configuration

Settings belong in `$CODEX_HOME/overmind.toml` (normally
`~/.codex/overmind.toml`). Keep the existing `[tui]` and `[pipelines]` tables when
adding this table. Stock Codex does not read this file.

```toml
[tool_selector]
enabled = true
model = "gpt-6-luna"
timeout_ms = 2000
max_tools = 8
min_probability = 0.55
cache_ttl_secs = 300
api_key_env = "OPENAI_API_KEY"
```

Selection is disabled by default. It requires a separate OpenAI API key, including
when the conversation uses Cursor or OpenAI subscription authentication. It first
checks `OPENAI_API_KEY`, then `$CODEX_HOME/secrets/openai.env`. The key file must be
owner-readable only on Unix (`chmod 600`); the file is parsed as data and is never
sourced as a shell script. Optional `key_file` and `api_key_env` settings can select
another private file or environment variable. Keep keys out of the project and
Git history.

The selector sends current-turn user text and complete eligible plugin tool
definitions to `https://api.openai.com/v1/decisions`. It excludes tool definitions
blocked by the existing exposure and permission policy. It does not send files,
images, previous conversation history, or tool results. Text added by steering is
included when selection runs again.

With direct tools, chosen definitions are exposed immediately. With OpenAI's
`code_mode_only` models, chosen declarations are included in the existing
`functions.exec` interface; MCP tools remain deferred and keep their execution
mode. Other tools remain available through ordinary lazy discovery (`ALL_TOOLS`
in native code mode; `tool_search` where enabled). An unchanged turn reuses its
selection; steering or changes to the catalog or selector settings invalidate it.

Missing credentials, an empty result, refusal, timeout, HTTP errors, invalid or
partial replies, and local resource limits all preserve ordinary lazy loading.
The deadline covers the whole selection operation. Local limits are 512 tools,
1 MiB of serialized input, batches of 64 questions and at most four concurrent
batches. These are Overmind resource limits, not advertised Decisions API limits.

Set `enabled = false` to return to plain lazy loading. Selector diagnostics use
the `overmind::tool_selector` tracing target and include counts, selected tool
names, timing, cache status, usage and a bounded failure reason. They omit user
text, schemas, endpoint credentials and API keys.

## TUI controls and progress

```toml
[tui]
hud = true
pipeline = true
pipeline_panel = true
activity = true
ascii = false
```

The pipeline panel shows a bounded window of stages around the active stage,
their real elapsed times, and expected or recorded artifact paths. Completed
artifacts are marked verified; changed evidence is marked stale. Paused and
finished clocks stop advancing. Narrow terminals show fewer rows and preserve
the composer. `/pipeline inspect` and `/pipeline status` provide the full view.

MCP servers that send `notifications/progress` now update the matching active
tool call. A positive, finite total produces a percentage bar. Reports without a
valid total display status and elapsed time. Overmind does not estimate a
percentage for a tool that reports none. Overlapping calls, cancellation and
late notifications are tracked by call identity. Progress is transient and is
not replayed from saved conversation history.

`ascii = true` or `TERM=dumb` selects ASCII glyphs. `NO_COLOR=1` disables color.
Set `pipeline_panel = false` to retain the compact stage row alone, or `hud =
false` to hide the HUD and panel.

## Validation tools

See the [2026-10-08 candidate report](validation-2026-10-08.md) for recorded
checks, build inputs and live comparison limits.

The harness uses the actual Overmind executable, isolated homes and a local MCP
fixture with 24 harmless fact tools. Fresh challenge values prove that correct
answers came from real tool calls. It records sanitized measurements and leaves
the installed Codex launcher and configuration unchanged.

```bash
node --test scripts/overmind/harness.test.mjs
node scripts/overmind/offline.mjs \
  --binary codex-rs/target/debug/codex \
  --output .codex/specs/decisions-tui/offline-check
node scripts/overmind/live-ab.mjs preflight
```

The offline run uses only owned loopback mock endpoints, including successful
selection and refusal, invalid-response, HTTP-error, empty-result and timeout
fallbacks. The preflight reports credential presence and build availability
without making inference requests or printing keys.

A live run makes paid provider and Decisions requests. Run it deliberately after
building the candidate and passing offline checks:

```bash
node scripts/overmind/live-ab.mjs run --confirm-live \
  --binary codex-rs/target/debug/codex --reps 2 --max-jobs 24 \
  --openai-model gpt-6.1-sol --cursor-model composer-2.5
```

Live jobs are sequential and bounded to eight Responses requests and 120 seconds
each. The three task types cover a single required tool, complementary tools and
similar distractor tools. Paired reports include actual calls, answer correctness,
wall time, model usage and selector usage. Small fixture measurements do not
establish a general speed or cost advantage. Missing counters remain unknown.

Cursor's exact SDK run counters are separate from its estimated current context.
SDK input, cache reads and cache writes are additive; OpenAI-compatible input
includes all three. See [Cursor accounting](cursor-context-cost.md) for the
distinction and the limitations of context estimates.
