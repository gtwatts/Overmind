# Overmind

Overmind is Todd Watts's customized build of the [Codex CLI](https://github.com/openai/codex).
It stays a thin, rebase-friendly layer on top of upstream Codex and adds the things Todd's
production work needs every day: video jobs, repeatable multi-stage pipelines, and a terminal UI
that shows what is going on.

The canonical public repository is [gtwatts/Overmind](https://github.com/gtwatts/Overmind).
Use its default branch, `overmind/main`, for Overmind; `main` remains the upstream Codex mirror.

## Vision

Codex is a strong general coding agent. Overmind turns it into a production cockpit:

- type `/video <brief>` and the agent loads the right pipelines, tools and reference examples;
- run a pipeline as a real thing with stages, dependencies and artifacts, and inspect or re-run
  any stage;
- watch progress in the TUI with progress bars, status indicators and per-stage state instead of
  scrolling logs.

## The three pillars

### 1. User-defined slash commands (M1)

Upstream Codex has no user-defined slash commands (`~/.codex/prompts` was removed). Overmind adds
them back as markdown files:

| Layer | Location | Notes |
| --- | --- | --- |
| Bundled | `codex-rs/tui/assets/overmind/commands/*.md` | compiled in; lowest precedence |
| User | `$CODEX_HOME/commands/*.md` (`~/.codex/commands`) | overrides bundled |
| Project | `<git root>/.codex/commands/*.md`, then `<cwd>/.codex/commands/*.md` | only for trusted projects; overrides user |

Built-in commands always win a name clash. The file stem is the command name (`video.md` ->
`/video`; lowercase letters, digits, `-`, `_`). Malformed files are skipped with a warning in the
transcript; they never crash the TUI.

No restart needed: the command directories are rescanned every time the `/` popup opens and
before a `/name ...` submission is checked, so a new or edited file shows up the next time you
type `/`. `/commands` lists every custom command with its source file, how its skills resolve,
and any skipped or malformed files with the reason.

Why `$CODEX_HOME/commands`: it sits next to the existing per-user Codex directories
(`skills/`, `rules/`, the former `prompts/`), honours `CODEX_HOME`, and mirrors the project-local
`.codex/` convention that Codex already uses for trusted project config. The name matches what
users of other agent CLIs expect (`.claude/commands`).

Format:

```markdown
---
description: start a video job of any style    # shown in the slash popup
argument-hint: "<who it's for / kind / style>"  # shown after the description
skills: [photocraft, blender]                    # attached as skills (see below)
files: [~/Documents/projects/ai-video-examples/index.md]   # listed for the agent to read
pipelines: [blender-motion-graphics, high-end-whiteboard]  # resolved to pipeline.yaml + PIPELINE.md
---
Brief: $ARGUMENTS
...instructions...
```

All frontmatter keys are optional; without frontmatter, the first line of the body is used as the
description. The body supports `$ARGUMENTS`, `$1`..`$9` and `$$`. If it uses no argument
placeholder, the arguments are appended under `## Request`. `/name free text` expands into the
user turn (body plus a short "Context for /name" block that lists skills, files and pipelines with
resolved paths) and is submitted like a normal message, so queueing, steering and history keep
working.

Skills named in `skills:` are attached the same way the `$` popup attaches them: as structured
skill inputs bound to the skill's `SKILL.md` path. This matters for plugin skills, which the
skills list names `<plugin>:<skill>` (`photocraft:photocraft`, `blender-mcp:blender`,
`gordon-skills:remotion-video-production`). Core only matches a plain `$name` text mention against
the exact full name, and the TUI mention parser stops at `:`, so a bare `$photocraft` in the text
never loaded the plugin skill. An entry may use the full name or the bare skill name; a bare name
resolves to `<plugin>:<name>` when exactly one plugin provides it (or the plugin is named after
the skill). Entries that are missing, disabled or ambiguous are listed in the context block and in
`/commands` instead of being silently dropped.

Pipelines named in `pipelines:` are looked up in `<project>/.codex/pipelines/<name>` (trusted
projects), `$CODEX_HOME/pipelines/<name>`, and the `pipelines/` folder of every plugin in
`$CODEX_HOME/local-marketplaces/*/plugins/*` (this is where Todd's `gordon-workflows` pipelines
live today). An entry containing `/` or starting with `~` is treated as a directory path.

Bundled starter commands (plain markdown, built from Todd's existing skills and pipelines; the
pipelines and skills themselves are not modified):

| Command | What it does | Skills / context |
| --- | --- | --- |
| `/video <brief>` | any video job (converted from the `$video` skill): Blender, Photocraft, Remotion/HTML, FFmpeg, default formats by style | photocraft, blender, remotion-video-production, ffmpeg-skill; example index; blender-motion-graphics, storyboard-pipeline-creator, high-end-whiteboard pipelines |
| `/whiteboard <brief>` | high-end whiteboard video through the `high-end-whiteboard` pipeline (Photocraft board craft, ImageMagick fallback, approval gates kept) | high-end-whiteboard, photocraft, imagemagick, whiteboard-animator, remotion-video-production; high-end-whiteboard pipeline |
| `/photocraft <asset>` | create or edit image assets with the Photocraft MCP tools or CLI, keeping editable masters | photocraft |
| `/examples <brief>` | search `~/Documents/projects/ai-video-examples` for matching references (answer only) | library README and index |

Copies live in `~/.codex/commands/` on watts, so they can be edited without rebuilding (a user
file overrides the bundled one with the same name).

### 2. Native pipelines (M2)

Pipelines, not "workflows": automation pipelines with stages, dependencies, artifacts, and the
ability to inspect and re-run a stage. M2 ports the format already used by the
`gordon-workflows` plugin:

- `pipeline.yaml` (JSON-compatible): `name`, `schemaVersion`, `description`, `inputs`,
  `steps[]` (`id`, `kind` = instruction | checkpoint | ..., `description`, `dependsOn[]`),
  `safety` (approval-required actions), `outputs[]`, `runtime`;
- `PIPELINE.md`: the human guide, domain notes and resume instructions;
- optional `prompts/`, `templates/`, `validators/` folders.

Integrated on `overmind/main` (originally developed on `overmind/m2-pipelines`):

1. **`codex-rs/overmind-pipelines` crate.** It reads both dialects found on watts:
   - Codex-native (gordon-workflows plugin): JSON text, explicit `dependsOn`, `actionHint`.
   - Pi (`~/.pi/agent/pipelines`): block YAML, implicit linear order, `action`, `requires`.

   It validates names, ids and dependencies. Ordering is a stable topological sort that reports
   cycles. Inputs are checked like `workflow.mjs` does: unknown, missing and mistyped values are
   errors, and defaults are applied.

   Run state lives in `<workspace>/.overmind/runs/<run-id>/`: `state.json` plus
   `pipeline.snapshot.yaml`. The directory is git-ignored, with modes 0700/0600. A run is pinned
   to its definition snapshot and its inputs. A stage is done when its declared output paths exist,
   and its evidence (sha256, size) is recorded. Changed evidence marks downstream stages stale.
   An ignored test parses all 39 real definitions on watts.
2. **`/pipeline` command.** Each stage becomes one queued user turn: a prompt with the stage task,
   role, skill, hints, inputs, upstream artifacts and expected outputs, with the stage's skill
   attached. When the turn ends, the outputs decide done or failed and the run auto-advances.

   Auto-advance stops at:
   - approval gates (`approvalRequired`, `human-review`), which need `/pipeline approve`;
   - missing outputs;
   - an interrupted turn;
   - `/pipeline stop`;
   - queued user messages;
   - `[pipelines] auto_advance = false` in `overmind.toml`.

   Subcommands:

   | Subcommand | What it does |
   | --- | --- |
   | `list` | List pipelines |
   | `run <name> [key=value ...] [free text]` | Start a run |
   | `resume` | Continue a run. A stage whose outputs appeared after an interruption is recorded, not re-run |
   | `approve` | Approve the current gate and run that stage |
   | `status [run]` | Show run progress |
   | `inspect <pipeline\|stage\|run>` | Show details |
   | `rerun <stage> [run]` | Re-run a stage and everything downstream |
   | `stop` | Stop auto-advance |

   Search roots: `<project>/.codex/pipelines` (nearest project root first),
   `$CODEX_HOME/pipelines`, then `$CODEX_HOME/local-marketplaces/*/plugins/*/pipelines`.
3. **HUD.** The HUD shows a pipeline row: a stage track or dots, done/total and the current stage
   (◇ awaiting approval, ↻ stale). The row narrows to `● 3/16` on small terminals.

### 3. Richer TUI (M3)

Visual feedback where Codex shows little today. The first set (branch `overmind/m3-tui`) is the
**HUD**: one row above the composer plus a per-turn summary line in the transcript. Code lives in
`codex-rs/tui/src/overmind/hud/`; ChatWidget events reach it through one-line hooks in
`chatwidget/overmind_hud.rs`.

HUD row, as width allows (segments step down to compact forms, then drop lowest priority first):

- **live turn activity** (only while a turn runs): elapsed time and the current phase (thinking,
  responding, running `<command>`, editing N files, calling `server.tool`, searching the web,
  generating an image, *awaiting approval* highlighted), plus tool counts by kind
  (`7 tools (4 sh, 2 edit, 1 mcp)`);
- **plan progress** from `update_plan`: `plan ━━━━━─── 3/5 ▸ <current step>`;
- **context gauge**: `ctx ━━━━━━──── 58% 148K/256K`, calm below 60%, warning from 60%, critical
  from 85%;
- **usage-limit bars** (5h / weekly) when the OpenAI account reports them (hidden for other
  providers);
- **provider badge** for non-OpenAI providers, e.g. `◆ Cursor grok-4.7`.

Turn summary (after each turn, below "Worked for ..."): tokens in (cached) → out (reasoning),
tool counts, plan, context, and a cost estimate when a price is configured.

Generic widgets for M2 (`overmind/hud/meter.rs`, `segments.rs`): half-cell progress bars with an
ASCII fallback, `progress(label, done, total)`, a stage track (`✓ plan  ● build  ○ test`) with
`StageStatus` glyphs, and the width-aware segment line composer.

Degradation: `TERM=dumb` (or `ascii = true`) switches to ASCII bars and glyphs; `NO_COLOR` (or no
color support) drops colors but keeps bold on warnings; narrow terminals keep the most important
segment. The HUD is off in upstream unit tests so upstream snapshots do not change.

Configuration lives in `$CODEX_HOME/overmind.toml`, not `config.toml`: stock Codex shares
`config.toml` and rejects unknown tables, so an `[overmind]` table there would break it. The
file's `[tui]` table is Overmind's `[overmind.tui]`:

```toml
[tui]
hud = true            # master switch for the HUD row
context_gauge = true
activity = true
plan_progress = true
pipeline = true
pipeline_panel = true # stage clocks and expected/verified artifacts
rate_limits = true
model_badge = true
turn_summary = true
ascii = false         # force ASCII bars

# optional cost estimates, USD per million tokens, keyed by model slug
[tui.prices."composer-2.5"]
input = 0.5
cached_input = 0.05
output = 2.5
```

Unknown keys under `[tui]` produce a transcript warning and fall back to defaults.

The pipeline row (M2) uses the stage widgets above; toggle it with `pipeline = true|false` under
`[tui]`. The bounded pipeline panel adds per-stage elapsed time and expected, verified or stale
artifact details; toggle it with `pipeline_panel = true|false`. Numeric MCP progress now reaches
the matching active call, with a percentage only when the server reports a valid total.
See [Decisions selection and TUI progress](docs/overmind/decisions-tui.md) for configuration,
fallback behavior and validation tools, and the
[refinement validation report](docs/overmind/refinement-2026-10-08.md) for the latest checks.
The [local candidate guide](docs/overmind/candidate-installation.md) covers versioned
development bundles, verification and a later trial or rollback.

### Per-turn Decisions tool selection

The optional OpenAI Decisions selector preloads relevant plugin MCP tool definitions for both
OpenAI and Cursor turns. It preserves existing tool permissions and execution modes, with the
remaining tools still available through lazy discovery. Failure or timeout returns to ordinary
lazy loading. Enable `[tool_selector]` in `overmind.toml` and supply an OpenAI API key; subscription
authentication alone does not authenticate the separate Decisions API.

### Shipped extra: built-in Cursor provider

`codex-rs/overmind-cursor` adds a `cursor` model provider: `overmind -m grok-4.7` (or
`grok-4.6`, `composer-2.5`; Gemini, Kimi, GLM and Muse once Cursor's usage pool allows) talks to
Cursor models through a bundled loopback Node helper while Codex keeps running its own tools.
The key comes from `CURSOR_API_KEY` or `$CODEX_HOME/secrets/cursor.env`.

Cursor models appear in `/model` (after the active provider's models) when a Cursor key is
configured. A thread cannot change provider, so picking a Cursor model on an OpenAI session (or
the reverse) starts a fresh session on that model; nothing is written to the shared
`config.toml`, which stock Codex also reads.

Why a trivial Cursor turn costs ~165K input tokens (plugin MCP schemas, cumulative usage) and what
to do about it: [docs/overmind/cursor-context-cost.md](docs/overmind/cursor-context-cost.md).

### Always the built-in app server

Overmind never adopts or starts the shared background daemon under `CODEX_HOME`: that daemon is
the stock Codex binary and lacks Overmind's providers and features. Every provider runs on the
embedded app server (`codex-rs/tui/src/overmind/server.rs`, hooked into
`startup_orchestration.rs` and the session subcommands). An explicit `--remote` endpoint is
still honored. `OVERMIND_SHARED_DAEMON=allow` restores the upstream behaviour; it exists so the
upstream daemon test suites keep exercising that code. Consequence: the agents overview only shows
Overmind's own sessions, not the stock daemon's.

## Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 | User-defined slash commands, bundled `/video` `/whiteboard` `/photocraft` `/examples`, skill attachment, live reload, `/commands`, tests | integrated on `overmind/main` |
| Extra | Built-in Cursor provider (`overmind-cursor`) | integrated on `overmind/main` |
| M2 | Native pipelines (port `pipeline.yaml` + `PIPELINE.md`), run state, inspect/re-run | integrated on `overmind/main` |
| M3 | TUI visuals: HUD, turn summary, pipeline stage panel and reported MCP progress | validated candidate ready for user trial; see validation report |
| M4 | Carry over Todd's setup | planned |

### M4: carry-over of Todd's current Codex setup

- `$video` skill (`~/.codex/skills/video/SKILL.md`): replaced by `/video` (M1). The skill stays
  in place for stock Codex.
- Motion-graphics example library (`~/Documents/projects/ai-video-examples`, 161+ examples with
  `index.md`, `index.json`, `patterns.md`, `html-video.md`): referenced by `/video`, and
  searchable with `/examples <brief>` (M1). M4 lets pipelines attach matching examples as context.
- Photocraft MCP plugin (`~/plugins/photocraft`): keep using it as a Codex plugin; M3/M4 show its
  job progress (`jobs_list`, `session_list`) in the TUI.
- Daily motion-graphics example search routine: keep it running outside the TUI; M4 turns it into
  an Overmind pipeline (`example-search`) so runs, results and failures are visible and
  re-runnable.
- clef-compact hook: **not carried over** (removed because it added tokens and latency). Optional
  maybe for later, only if a new version proves it saves tokens.

## Staying rebased on upstream

Remotes in the local clone (`~/Documents/projects/Overmind`):

- `upstream` = `https://github.com/openai/codex.git` (fetch only; push is disabled);
- `origin` = `https://github.com/gtwatts/Overmind.git` (public; pushes go over SSH because the
  `gh` token has no `workflow` scope).

Branches:

- `main` mirrors `upstream/main` exactly. Never commit on it.
- `overmind/main` is Overmind's trunk (the default branch on GitHub).
- `overmind/next` and the existing milestone branches preserve earlier development history;
  their active work has been integrated into `overmind/main`.
- Start new `overmind/<milestone>-<topic>` feature branches from `overmind/main`, review them,
  then merge them into `overmind/main`.
- `clef-context-filter` is an archived experiment; do not build on it.

Keep one local project directory, `~/Documents/projects/Overmind`. Demo assets live in
`docs/overmind/media/`. See [the consolidation record](docs/overmind/consolidation.md).

Sync routine:

```bash
git fetch upstream
git checkout main && git merge --ff-only upstream/main && git push origin main
git checkout overmind/main && git merge main
```

Rules that keep conflicts small:

1. Overmind logic lives in Overmind modules: `codex-rs/tui/src/overmind/`,
   `codex-rs/tui/src/chatwidget/overmind_commands.rs`,
   `codex-rs/tui/src/bottom_pane/chat_composer/overmind_commands.rs`,
   `codex-rs/tui/assets/overmind/`, and new
   `overmind-*` crates. Upstream files only get small hooks that call into them.
2. Do not reformat or reorganize upstream code; keep diffs in upstream files minimal and
   mechanical (an extra enum variant, an extra parameter, a one-line call).
3. Prefer new crates over growing `codex-core`.
4. Keep tests next to the Overmind modules (`*_tests.rs`) so upstream test files rarely change.
5. After each sync, run `just fmt`, `just test -p codex-tui` and the Overmind tests before
   pushing.

### Upstream files touched by M1

- `codex-rs/tui/src/lib.rs`: `mod overmind;`
- `codex-rs/tui/src/bottom_pane/slash_commands.rs`: `SlashCommandItem::Custom`, custom list
  parameter on the lookup helpers
- `codex-rs/tui/src/bottom_pane/command_popup.rs`: `CommandItem::Custom` rows
- `codex-rs/tui/src/bottom_pane/chat_composer.rs` and `chat_composer/slash_input.rs`: hold the
  custom command list, Enter/Tab behaviour for custom rows
- `codex-rs/tui/src/bottom_pane/mod.rs`: setter/getter passthrough
- `codex-rs/tui/src/chatwidget.rs`, `chatwidget/constructor.rs`, `chatwidget/session_flow.rs`,
  `chatwidget/input_flow.rs`, `chatwidget/slash_dispatch.rs`: load commands, expand submissions
- `codex-rs/tui/Cargo.toml` (+ `Cargo.lock`): `serde_yaml` for frontmatter
- `codex-rs/tui/src/slash_command.rs`: the `Commands` variant (`/commands`) and its availability
  lists; `bottom_pane/slash_commands.rs` test list updated for it
- `codex-rs/tui/src/bottom_pane/chat_composer.rs`: `mod overmind_commands;`, the env field, and
  two one-line rescan hooks (popup open, slash submission); the logic is in
  `chat_composer/overmind_commands.rs`

### Upstream files touched by M3 (HUD) and the Cursor routing hook

- `codex-rs/tui/src/overmind/mod.rs`: `mod hud;`
- `codex-rs/tui/src/chatwidget.rs`: `mod overmind_hud;`, context sync after token updates
- `codex-rs/tui/src/chatwidget/constructor.rs`: `overmind_hud_init()`
- `codex-rs/tui/src/chatwidget/turn_runtime.rs`: turn start/finish and plan hooks
- `codex-rs/tui/src/chatwidget/{streaming,command_lifecycle,tool_lifecycle,tool_requests,dynamic_activity}.rs`:
  one-line activity events
- `codex-rs/tui/src/chatwidget/rate_limits.rs`, `status_surfaces.rs`: limit and badge sync
- `codex-rs/tui/src/bottom_pane/mod.rs`: the `overmind_hud` field, an accessor, one row pushed
  above the composer, and the test module registration
- `codex-rs/tui/Cargo.toml` (+ `Cargo.lock`): `codex-overmind-cursor`

### Upstream files touched by M2, the embedded-server default and the `/model` extra

- `codex-rs/Cargo.toml` (+ `Cargo.lock`): the `overmind-pipelines` member;
  `codex-rs/tui/Cargo.toml`: `codex-overmind-pipelines`
- `codex-rs/tui/src/slash_command.rs`: the `Pipeline` variant (inline args, available during a
  task); `chatwidget/slash_dispatch.rs`: two dispatch arms and the queue-drain arm
- `codex-rs/tui/src/chatwidget.rs`, `chatwidget/constructor.rs`: `overmind_pipelines` and
  `overmind_next_session_model` fields, `overmind_pipelines_init()`
- `codex-rs/tui/src/chatwidget/turn_runtime.rs`, `chatwidget/input_restore.rs`: stage hooks on
  turn start/finish/interrupt
- `codex-rs/tui/src/startup_orchestration.rs`: daemon exclusion wrapped by
  `overmind::server::daemon_exclusion`, no warning and a `overmind_embedded` selection reason
  for it; the earlier Cursor-only special case is gone
- `codex-rs/tui/src/session_archive_commands.rs`: session subcommands do not reuse the daemon
- `codex-rs/tui/tests/suite/focus_palette.rs`: PTY helper sets `OVERMIND_SHARED_DAEMON=allow` for
  upstream tests; one Overmind test launches without it
- `codex-rs/tui/src/app_server_session.rs`: Cursor presets appended to the model list
- `codex-rs/tui/src/chatwidget/model_popups.rs`, `session_model_selection.rs`,
  `app_event.rs`, `app/event_dispatch.rs`, `app/session_lifecycle.rs`: cross-provider picks start a
  fresh session (`AppEvent::OvermindStartModelSession`)

## Building and trying Overmind side by side

Overmind builds the same `codex` binary as upstream. Do not install it over the stock Codex; run
it from the target directory under another name:

```bash
cd ~/Documents/projects/Overmind/codex-rs
cargo build -p codex-cli -p codex-code-mode-host --bins
alias overmind="$HOME/Documents/projects/Overmind/codex-rs/target/debug/codex"
overmind            # then type / to see /video, /whiteboard, /photocraft, /examples; /commands lists them
overmind -m grok-4.7   # Cursor model; the HUD shows "◆ Cursor grok-4.7"
overmind               # then /pipeline list, /pipeline run <name> ...
```

The HUD row appears above the composer once there is something to show (a running turn, a plan,
context usage after the first turn, or a non-OpenAI provider). Try `TERM=dumb overmind` or
`NO_COLOR=1 overmind` to see the fallbacks.

The `codex-code-mode-host` companion must stay beside the source-built CLI for native OpenAI
code-mode models. This side-by-side command does not replace the installed `codex` launcher.
