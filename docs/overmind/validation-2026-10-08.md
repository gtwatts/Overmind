# Decisions and TUI candidate validation — 2026-10-08

This records the first candidate. See the [subsequent refinement report](refinement-2026-10-08.md)
for the latest recovery fixes, real coding checks and versioned package.

This candidate adds shared per-turn Decisions selection, a pipeline stage panel,
actual MCP numeric progress, and corrected Cursor cache accounting. Validation
uses isolated homes and harmless MCP fixtures. The installed Codex launcher and
active user configuration are unchanged.

## Automated checks

| Area | Passing checks |
| --- | ---: |
| Selector, cache, fallback and native code-mode exposure | 22 |
| Core tool-policy regressions | 60 |
| Overmind TUI, call correlation and composer groups | 80 + 2 + 4 |
| MCP client unit suite | 259 |
| Native MCP transport integration suites | 57 |
| Progress exclusion from history | 2 |
| Protocol compatibility suite | 321 |
| Cursor provider and pipeline lifecycle | 14 + 25 |
| Cursor helper contracts and runtime | 307 |
| Node validation harness | 18 |
| Python generated-protocol contracts | 5 |

These are suite counts, with some overlapping targeted cases; they are not a
count of unique tests. Five environment-dependent or pre-existing tests were
ignored across the MCP, protocol and pipeline groups. Rust tests use the
repository's prescribed `RUST_MIN_STACK=8388608`. An initial broader core run
without that setting exceeded the test thread's stack; the prescribed run passed.

Helper type checking and its server build passed. Stable and experimental
app-server protocol bundles, JSON/TypeScript schemas and Python SDK types were
regenerated. Rust formatting and `git diff --check` passed.

## Build inputs

The Linux candidate includes the CLI, code-mode host and Linux sandbox companion.
The pinned V8 dependency's default sandbox archive download returned HTTP 404.
The local build therefore uses the exact cached archive and bindings used by the
earlier working host, retaining the sandbox feature. Archive SHA-256:
`82e3368ef77a5427d781104c3903fcffadfaef1fc324d55162cfa598a509e927`.
The binding hash matches the published V8 150.4.0 release binding; the cached
archive's original distribution source was not independently authenticated.
Fresh machines need matching archive/bindings via `RUSTY_V8_ARCHIVE` and
`RUSTY_V8_SRC_BINDING_PATH`, or a working upstream/source V8 build.

## End-to-end and live checks

The actual executable passed all 14 offline scenarios: plain, selected, empty,
HTTP failure, timeout, malformed response and refusal on native Cursor Direct
and OpenAI CodeModeOnly. Each case verified initial schema exposure, native
discovery, a real MCP call and a fresh exact answer. The harness reads native
Responses Lite `additional_tools` declarations as well as top-level tools.

Four live smoke jobs and all 24 main comparison jobs passed. The main study used
24 read-only fixture tools, three task types, two repetitions and balanced arm
order. Every Decisions job made a successful `gpt-6-luna` request and exposed all
required definitions before the first conversation-model request: 12/12. All
answers matched fresh tool-produced facts; no unrelated MCP tools were called.

The table uses the main study only. Deltas are median paired **Decisions minus
plain** across six pairs per provider. Conversation token deltas exclude the
separate selector overhead shown below.

| Measurement | OpenAI `gpt-6.1-sol` | Cursor `composer-2.5` |
| --- | ---: | ---: |
| Correct live jobs | 12/12 | 12/12 |
| Native discovery | `ALL_TOOLS` in code mode | `tool_search` |
| Responses requests per job, plain → Decisions | 3 → 2 | 3 → 2 |
| Median elapsed time, plain → Decisions | 14.620s → 7.643s | 16.860s → 11.570s |
| Median paired elapsed delta | −5.309s | −5.465s |
| Conversation input-token delta | −14,927 | unknown |
| Conversation cached-input delta | −12,580.5 | unknown |
| Conversation uncached-input delta | −2,337.5 | unknown |
| Conversation output-token delta | −19 | unknown |
| Pairs with complete conversation usage | 6/6 | 0/6 |
| Additional selector input tokens, median | 8,405 | 8,403 |
| Selector duration, median | 389ms | 310ms |
| Selector duration range | 230–700ms | 218–880ms |

The API reported zero selector output tokens in these runs. Selector costs are
separate from conversation-model costs; token counts across different models do
not establish a dollar saving. Two OpenAI complementary-tool pairs were slower
with selection, and one Cursor pair was slightly slower. This small fixture
study supports correctness and the observed discovery reduction, not a general
speed, cost or quality guarantee.

Cursor's plain discovery path changes its executable tool catalog and starts a
replacement SDK run. The final raw counters cover that replacement, leaving the
initial discovery run without complete usage. All six plain Cursor jobs are
therefore partial; the six selected jobs have complete counters. Paired full
Cursor token/cost deltas remain unknown. Additive cache conversion is correct
for each reported SDK run; it does not reconstruct missing cancelled-run usage.

Jobs used isolated homes/projects, a loopback HTTP observer, at most eight
Responses requests and a 120-second limit. HTTP 426 selected native HTTP fallback
immediately; WebSocket performance was not measured. Keys, request bodies, raw
responses and reasoning were not persisted by the observer.

The executable used for this live study had SHA-256
`209c231a1f3dc2c91fb39a89ac53de90873e73ec4a966d35e1c3df550f4667d4`.
The final candidate differs only in TUI completion/status retention and its
lifecycle regression. Selector, provider, progress transport and helper behavior
are unchanged from the live study.

## Final terminal checks and candidate

The rebuilt candidate passed the complete 14-case offline matrix again.
Actual interactive terminal inspection verified server-reported progress,
paused approval, retained completed stage/artifact details, fixed final clocks,
and stale evidence after changing a synthetic artifact. Color and ASCII with
`NO_COLOR=1` were inspected in wide recordings. Direct 24×12 and 80×8 terminal
captures verified that progress and stage status remain visible and typed
composer drafts remain usable while paused and completed.

The UI fixture used dummy credentials, owned loopback mocks and precreated
synthetic output files. It verifies presentation and artifact checks; those
files are not agent-produced deliverables. A lifecycle regression also verifies
that completion releases execution state, a new run replaces the final panel,
and inspecting an older run cannot replace another active run's panel.

Final CLI SHA-256:
`a0bfb414139bbf2c3090a76de659ab47b29abc3abd547b649905a0fb9c33da34`.
The development build reports `codex-cli 0.0.0`. The locally prepared candidate
keeps both built companions beside the CLI and pins the tested Cursor helper.
It is ready for the user's trial; replacing the installed Codex is a later step.

## Trying the candidate

The source-tree executable is `codex-rs/target/debug/codex`. Keep its freshly
built `codex-code-mode-host` and `codex-linux-sandbox` companions alongside it.
Set `OVERMIND_CURSOR_HELPER_DIR` to this checkout's
`codex-rs/overmind-cursor/helper` to use the tested helper rather than an older
installed copy. The private candidate launcher does this automatically.

See [configuration and controls](decisions-tui.md). Selection is opt-in through
`$CODEX_HOME/overmind.toml`; it needs a separate OpenAI API key even for Cursor
conversations. Disabled or failed selection preserves native lazy discovery.
Cursor context remains an estimate; exact SDK run usage is recorded separately.
