# Overmind

Overmind is Todd Watts's customized build of the [Codex CLI](https://github.com/openai/codex).
It stays a thin, rebase-friendly layer on top of upstream Codex and adds the things Todd's
production work needs every day: video jobs, repeatable multi-stage pipelines, and a terminal UI
that shows what is going on.

The repository is **private while Overmind is tested in daily use**. It may be open-sourced later,
once it has proven itself.

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

Why `$CODEX_HOME/commands`: it sits next to the existing per-user Codex directories
(`skills/`, `rules/`, the former `prompts/`), honours `CODEX_HOME`, and mirrors the project-local
`.codex/` convention that Codex already uses for trusted project config. The name matches what
users of other agent CLIs expect (`.claude/commands`).

Format:

```markdown
---
description: start a video job of any style    # shown in the slash popup
argument-hint: "<who it's for / kind / style>"  # shown after the description
skills: [photocraft, blender]                    # appended as $skill mentions
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

Pipelines named in `pipelines:` are looked up in `<project>/.codex/pipelines/<name>` (trusted
projects), `$CODEX_HOME/pipelines/<name>`, and the `pipelines/` folder of every plugin in
`$CODEX_HOME/local-marketplaces/*/plugins/*` (this is where Todd's `gordon-workflows` pipelines
live today). An entry containing `/` or starting with `~` is treated as a directory path.

`/video` ships as the first bundled command (converted from the `$video` skill). It covers
Blender for 3D, Photocraft for assets, Remotion/HTML for storyboarded composition, FFmpeg for
audio sync, and default formats by style. A copy lives in `~/.codex/commands/video.md` on watts,
so it can be edited without rebuilding.

### 2. Native pipelines (M2)

Pipelines, not "workflows": automation pipelines with stages, dependencies, artifacts, and the
ability to inspect and re-run a stage. M2 ports the format already used by the
`gordon-workflows` plugin:

- `pipeline.yaml` (JSON-compatible): `name`, `schemaVersion`, `description`, `inputs`,
  `steps[]` (`id`, `kind` = instruction | checkpoint | ..., `description`, `dependsOn[]`),
  `safety` (approval-required actions), `outputs[]`, `runtime`;
- `PIPELINE.md`: the human guide, domain notes and resume instructions;
- optional `prompts/`, `templates/`, `validators/` folders.

Planned shape:

1. `codex-rs/overmind-pipelines` crate (new crate, so `codex-core` does not grow): parse and
   validate `pipeline.yaml`, build the stage DAG, detect cycles, topological order.
2. Run state in `<workdir>/.overmind/runs/<run-id>/state.json`: per-stage status (pending,
   running, done, failed, skipped), timestamps, artifacts, verifier notes. State is append-only
   so runs can resume after interruption.
3. TUI commands: `/pipeline list`, `/pipeline run <name> [inputs]`, `/pipeline status`,
   `/pipeline inspect <stage>`, `/pipeline rerun <stage>` (re-runs the stage and everything that
   depends on it). Execution is agent-orchestrated: each stage becomes a bounded user turn that
   carries the stage instructions, its inputs and upstream artifacts.
4. Approval gates from `safety.approvalRequiredFor` map to Codex approvals.
5. Slash commands can already reference pipelines (M1); in M2 they can also start one.

### 3. Richer TUI (M3)

Visual feedback where Codex shows little today:

- pipeline panel: stage list with status glyphs, a progress bar for the run, elapsed time per
  stage, and the current artifact;
- status indicators for long tool calls (render/encode jobs, MCP calls) with determinate progress
  when the tool reports it;
- token/context gauge and turn timeline in the status line;
- snapshot-tested ratatui widgets in `codex-rs/tui/src/overmind/` (styling per
  `codex-rs/tui/styles.md`).

## Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 | User-defined slash commands, bundled `/video`, tests | in progress on `overmind/m1-slash-commands` |
| M2 | Native pipelines (port `pipeline.yaml` + `PIPELINE.md`), run state, inspect/re-run | planned |
| M3 | TUI visuals: pipeline panel, progress bars, status indicators | planned |
| M4 | Carry over Todd's setup | planned |

### M4: carry-over of Todd's current Codex setup

- `$video` skill (`~/.codex/skills/video/SKILL.md`): replaced by `/video` (M1). The skill stays
  in place for stock Codex.
- Motion-graphics example library (`~/Documents/projects/ai-video-examples`, 161+ examples with
  `index.md`, `index.json`, `patterns.md`, `html-video.md`): referenced by `/video` today; M4 adds
  a `/examples <style or model>` command and lets pipelines attach matching examples as context.
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
- `origin` = `https://github.com/gtwatts/Overmind.git` (private; pushes go over SSH because the
  `gh` token has no `workflow` scope).

Branches:

- `main` mirrors `upstream/main` exactly. Never commit on it.
- `overmind/main` is Overmind's trunk (the default branch on GitHub).
- `overmind/<milestone>-<topic>` feature branches are reviewed by Todd, then merged into
  `overmind/main`.
- `clef-context-filter` is an archived experiment; do not build on it.

Sync routine:

```bash
git fetch upstream
git checkout main && git merge --ff-only upstream/main && git push origin main
git checkout overmind/main && git rebase main   # or merge, if the branch is shared
```

Rules that keep conflicts small:

1. Overmind logic lives in Overmind modules: `codex-rs/tui/src/overmind/`,
   `codex-rs/tui/src/chatwidget/overmind_commands.rs`, `codex-rs/tui/assets/overmind/`, and new
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

## Building and trying Overmind side by side

Overmind builds the same `codex` binary as upstream. Do not install it over the stock Codex; run
it from the target directory under another name:

```bash
cd ~/Documents/projects/Overmind/codex-rs
cargo build -p codex-cli --bin codex
alias overmind="$HOME/Documents/projects/Overmind/codex-rs/target/debug/codex"
overmind            # then type /video <brief>
```
