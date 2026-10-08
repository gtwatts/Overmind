# Overmind selector validation

These dependency-free Node scripts validate the actual built Overmind CLI in an
isolated `CODEX_HOME` and project. They do not install a binary or change the
normal Codex configuration. The fixture is a local plugin with 24 harmless,
read-only MCP tools and fresh ticket-dependent answers.

## Offline verification

```sh
node --test scripts/overmind/harness.test.mjs
node scripts/overmind/offline.mjs --binary /absolute/path/to/codex
```

The offline matrix uses owned loopback model and Decisions endpoints, dummy
credentials, and real native CLI/MCP execution. It covers plain lazy discovery,
selected definitions, valid empty selection, HTTP errors, timeout, malformed
answers, and refusal in two modes:

- Cursor `composer-2.5`: native Direct tools.
- OpenAI `gpt-6.1-sol`: native CodeModeOnly with selected declarations in
  `exec.description` and real `tools.mcp__fixture__…` execution.

There is no tool-mode override. Each case checks actual prompt exposure, selector
telemetry, native lazy discovery, an audited MCP call, and the exact fresh answer.
Direct mode uses native `tool_search`; CodeModeOnly uses `ALL_TOOLS` inside
`exec` to discover declarations before calling the tool. Catalog discovery
attempts and native tool-search calls are counted separately.
Use `--cases plain,selected` or `--modes code_mode_only` for a narrow rerun. Output
defaults to a fresh private directory in `/tmp`.

`startMockApis({ scenario, ticket, seed, taskId: 'progress' })` also supports a
two-second report tool for interactive TUI verification. The MCP server emits
four numeric progress notifications when the native client sends a progress
token. The normal benchmark tasks do not add that delay.
The mock defaults to eight model requests. Multi-turn terminal fixtures may set
`maxModelRequests` up to 24; the CLI benchmark and live per-job limit stay at eight.

## Bounded live comparison

The preflight reads credential presence without printing values or issuing model
requests:

```sh
node scripts/overmind/live-ab.mjs preflight \
  --binary /absolute/path/to/codex --source-home /path/to/existing/codex-home
```

Run live inference only after scheduling the bounded validation. A four-job smoke
uses one task, two providers, and both arms:

```sh
node scripts/overmind/live-ab.mjs run --confirm-live \
  --binary /absolute/path/to/codex --source-home /path/to/existing/codex-home \
  --tasks single --reps 1 --max-jobs 4
```

A two-repetition comparison across all three tasks runs 24 jobs:

```sh
node scripts/overmind/live-ab.mjs run --confirm-live \
  --binary /absolute/path/to/codex --source-home /path/to/existing/codex-home \
  --reps 2 --max-jobs 24
```

Each job has at most eight Responses requests, a 120-second timeout, and at most
four observed MCP calls. Jobs run sequentially and stop at the first failure.
`--providers openai`, `--arms decisions`, `--tasks complementary,misleading`,
`--openai-model`, `--cursor-model`, `--timeout-ms`, `--max-requests`, and `--output`
can narrow the run. The plan rejects more than 24 jobs. Arm order reverses on the
second repetition, and each pair shares its own fresh ticket and private seed.

The live defaults are OpenAI `gpt-6.1-sol`, Cursor `composer-2.5`, and Decisions
`gpt-6-luna`. Provider model metadata determines native tool mode. The selector
uses its normal two-second timeout and lazy fallback. Correctness requires both
the exact answer and successful required MCP calls with the matching ticket;
the prompt never contains the expected answer. Unrelated tool calls fail a job.

## Observation and artifacts

Native provider identifiers remain `openai` and `cursor`. OpenAI uses the native
`openai_base_url` hook through a loopback observer with the fixed official
`https://api.openai.com/v1/` destination. Cursor uses
`OVERMIND_CURSOR_BASE_URL` through the observer to a runner-owned instance of the
bundled helper. The runner terminates its CLI and helper process groups and
closes its listeners after each job, including failures.

Observation uses the native HTTP fallback. The observer and offline mock return
HTTP426 immediately to a WebSocket handshake on `GET /v1/responses`, avoiding
artificial retry delays. Provider identity, model metadata, tool mode, and POST
request content remain native. The manifest records `transport: "http_fallback"`;
these timings do not measure WebSocket performance. Native Responses Lite tool
declarations in developer `additional_tools` input items are measured alongside
ordinary top-level `tools` without inspecting message text.

Credentials come from existing `OPENAI_API_KEY` / `CURSOR_API_KEY` environment
variables or literal assignments in `secrets/openai.env` / `secrets/cursor.env`
under the source home. They are used only in process memory/environment. OAuth
state is never copied. No keys appear in fixture configuration. The observer
does not persist headers, model request bodies, raw responses, or reasoning.

Live output defaults to `.codex/specs/decisions-tui/live-<timestamp>/`, with private
directory/file modes. `manifest.json` records binary SHA-256, models, limits,
and the plan. Each job records sanitized CLI events, harmless MCP audit entries,
whitelisted request measurements, and a summary. `results.json` contains paired
differences, always Decisions minus plain. Selector cost is reported separately.
Missing selector counters remain `null`; fallback or interrupted selector
attempts cannot prove complete selector cost.

OpenAI input tokens include cached tokens; uncached input is input minus cache
reads. Cursor SDK `inputTokens`, `cacheReadTokens`, and `cacheWriteTokens` are
additive, as defined by the installed SDK's `toTokenUsage` / `sumTokenUsage`.
Cursor normalized total input is their sum; uncached input is SDK input plus
cache writes. The runner retains all raw SDK components and excludes estimated
intermediate Responses usage from final totals. Missing usage remains `null`;
interrupted runs may have incomplete totals and cannot support paired token
claims. Timings and the small fixture sample do not establish a general speed
or quality improvement.
