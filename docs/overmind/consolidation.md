# Repository consolidation — October 8, 2026

The canonical repository is [gtwatts/Overmind](https://github.com/gtwatts/Overmind), with
`overmind/main` as the default branch. Keep the local checkout at
`~/Documents/projects/Overmind`.

## What was preserved

- The active source at `befc6648316b1fac9bc6c91d40d7c40e59d0637c` already contained the
  slash commands, Cursor provider, HUD, pipelines, on-demand plugin tool loading, and Cursor
  usage work. The active development branches share this history; integration preserves the
  original commits.
- Five uncommitted Cursor helper files contained context-estimation work. Their original
  patch was backed up, a tool-batch accounting defect was repaired, and the existing contract
  tests were updated to exercise the new behavior. Exact SDK totals remain separate from
  protocol context estimates so ledger receipts retain full run usage.
- All seven original media/tape files are in [media/](media/README.md), with identical SHA-256
  hashes to their originals.
- `clef-context-filter` remains an archived experiment on its existing branch. It was already
  in the canonical repository and was not promoted into the active build.
- Existing local `AGENTS.md` and `.codex/` configuration remain local.

## Duplicate audit

`overmind-public-fork-unused` was an upstream OpenAI Codex fork with no Overmind-specific
commits, pull requests, issues, or releases to transfer. Of its 7,595 branch/tag refs, 7,592
matched the live upstream refs exactly. Its `main` and `overmind/main` pointed to the upstream
baseline already in this repository. Its older `latest-alpha-cli` pointed to upstream's
`rust-v0.162.0-alpha.18` release. The fork's project refs and metadata were preserved locally
before removal.

`Overmind-cursor` was a clean linked worktree at
`8c30d48e150cebe5c128f4bffd45e332b065f284`, an ancestor of the active checkout. It contained
no unique source or configuration. Its approximately 30 GB of ignored files were compiled
outputs, installed dependencies, virtual environments, and caches. The canonical checkout
already had its own newer build. Host process inspection found no processes using either
duplicate directory before cleanup.

## Verify the source

```sh
cd codex-rs/overmind-cursor/helper
npm ci
npm run typecheck:server
npm run build:server
npm test
cd ../..
cargo test -p codex-overmind-cursor -p codex-overmind-pipelines
cargo test -p codex-tui --lib overmind
cargo build -p codex-cli --bin codex
./target/debug/codex --version
```

The vendored helper contains the server only. Its inherited `typecheck:web` and `build:web`
scripts refer to an omitted web frontend; use the server-specific commands above.

## Local recovery

The untracked, Git-excluded `.local/consolidation-20261008/` directory contains the complete
pre-consolidation Git bundle, original uncommitted patch, local configuration copies, artifact
copies and hashes, ref inventories, and GitHub audit metadata. These files are not published.

Verify the backup with:

```sh
git bundle verify .local/consolidation-20261008/repository-before.bundle
```

For source recovery, clone that bundle into a temporary directory and inspect its branches.
The original `changes.patch` can then be applied to its original `overmind/cursor-context`
revision. Restore artifact files from the backup's `artifacts/` directory if needed. The
discarded worktree's caches can be rebuilt; they are not source backups.

To undo committed consolidation changes, use `git revert` on the consolidation commits from
`overmind/main` and review the resulting diff. Do not reset or force-push shared history.
