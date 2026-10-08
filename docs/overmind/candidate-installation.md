# Local development candidate

`scripts/overmind/package-candidate.mjs` packages existing built and tested files
for a private, side-by-side trial. It does not build, install dependencies,
download anything, read credentials, activate Codex, change `PATH`, or modify the
active `CODEX_HOME`. Node.js 22.19.0 or newer and a Linux x64/arm64 host are required.

This is a **development candidate**. The current CLI identifies itself as
`codex-cli 0.0.0`; the workflow accepts the debug profile only. Checksums identify
the bundled bytes. They do not authenticate the build or the cached V8 archive,
whose origin has not been independently authenticated. Unmodified debug binaries
can contain compiler source paths/debug information. Generated metadata and
launchers contain no absolute checkout paths. Keep the bundle private; this is
not a published binary distribution.

The source commit, clean/dirty snapshot and validation record are explicit caller
attestations. The packager does not run Git or infer that a dirty source tree was
built or tested. Use `clean` only when the tracked source exactly matches the
specified commit; otherwise use `dirty`. Neither label replaces rebuilding and
testing the files being packaged.

Run packaging after the CLI, code-mode host, Linux sandbox and Cursor helper
server build/tests have passed. Set a new version and the full source commit:

```bash
candidate_version='refinement-20261008-dev1'
source_commit='REPLACE_WITH_THE_FULL_40_CHARACTER_COMMIT'
candidate_dir="/tmp/overmind-candidates/$candidate_version"
node scripts/overmind/package-candidate.mjs pack \
  --version "$candidate_version" \
  --source-commit "$source_commit" \
  --source-snapshot dirty \
  --validation-record docs/overmind/validation-2026-10-08.md \
  --output "$candidate_dir"
node scripts/overmind/package-candidate.mjs verify "$candidate_dir"
```

`--help` lists input overrides. Defaults are this checkout,
`codex-rs/target/debug`, and `codex-rs/overmind-cursor/helper`. Output must be new
and disjoint from the source/build/helper trees and active `CODEX_HOME`; directory
symlinks in the output path are refused. Packaging uses a private sibling staging
directory and exclusive publication lock, verifies the completed staging tree,
then renames it into place on the same filesystem. Failures remove owned staging
files, and an existing candidate is never intentionally replaced. If an interrupted
process leaves a `.publish-lock` directory, confirm no packager is running before
removing that lock and retrying with a fresh version.

The bundle contains:

```text
overmind.mjs                  relocatable development launcher
bin/codex                     unchanged built CLI
bin/codex-code-mode-host       unchanged code-mode companion
bin/codex-linux-sandbox        unchanged Linux sandbox companion
cursor-helper/                entry, server dist, package metadata and notices
cursor-helper/node_modules/   installed production dependency closure
runtime/candidate.mjs          self-contained local verifier
candidate-manifest.json       source/build/runtime metadata and every payload hash
checksums.sha256              payload and manifest SHA-256 index
```

The helper must already have `overmind-entry.mjs`, `dist/index.js`, licenses and
installed dependencies matching its package lock. Required dependencies and
nested versions are preserved, including package JSON, runtime data, WASM/native
assets, and the matching Cursor SDK platform package's ripgrep, sandbox and
tree-sitter bindings. Development dependencies, helper source/tests, source maps,
type declarations, environment files and credential filenames are omitted.
Contained runtime links are materialized; escaping or cyclic links are refused.
The supported dependency layout is the existing npm-style installation; external
workspace links are refused rather than copied. Package metadata retains the
upstream licenses and lock integrity identifiers, which are not independent
authentication of installed files.

All output directories are private (`0700`); files are `0600`, or `0700` when
executable. Source file permissions and contents are unchanged. The same input
bytes, executable bits, Node version and metadata produce identical manifests
and checksum inventories at different output paths. No wall-clock packaging
timestamp or checkout path enters that inventory.

`verify` hashes every payload file and checks companion completeness, unexpected
files, permissions, Linux executable architecture and the runtime dependency
closure. Run it before each trial, after copying/moving a bundle, and before any
later activation:

```bash
node "$candidate_dir/runtime/candidate.mjs" verify "$candidate_dir"
```

The launcher performs **structural preflight**, checking file inventory, sizes,
permissions, companion headers, runtime packages and Node compatibility. It does
not rehash the large debug executables on every startup; same-size content changes
require the full `verify` command to detect. Checksums are local integrity evidence,
not signatures, and cannot protect against someone replacing both payload and
manifest.

Node itself is supplied by the host, not bundled. The executable launcher uses
`node` on the caller's `PATH`, requires the recorded minimum version, pins
`OVERMIND_NODE` to that process's Node executable and pins
`OVERMIND_CURSOR_HELPER_DIR` inside the relocated bundle. It adds `--no-daemon`
when that option is absent before the `--` positional boundary, keeping the
candidate process independent of an installed Codex daemon. Its
paths resolve through launcher symlinks; moving the checkout cannot redirect it
to a different helper. It preserves the caller's working directory and arguments.

For an isolated trial, create a fresh `CODEX_HOME` and deliberately configure only
the intended local fixtures or trial provider there. No real auth is copied by
this workflow. Supplying no trial configuration can expose ordinary Codex
onboarding; do not log in merely to make an offline fixture pass.

```bash
trial_home=$(mktemp -d /tmp/overmind-trial-home.XXXXXX)
chmod 700 "$trial_home"
CODEX_HOME="$trial_home" "$candidate_dir/overmind.mjs" --version
```

Full provider/MCP/plugin execution remains an integration check by the operator.
The packager's tests use harmless fixture executables and synthetic runtime data;
they cannot establish provider authentication, model accuracy, sandbox behavior,
native library compatibility or release provenance.

After an accepted trial, **activation requires a separate, explicit instruction**.
Do not run the following commands as part of packaging/preflight. Move the
verified bundle into a stable private versioned directory first; `/tmp` may be
cleaned. Keep the original executable or symlink, stage the replacement beside the
active path, then atomically rename it. This example is for GNU/Linux and an
existing user-owned `~/.local/bin/codex`; use a fresh activation identifier:

```bash
set -euo pipefail
stable_candidate="$HOME/.local/share/overmind/candidates/$candidate_version"
active_codex="$HOME/.local/bin/codex"
activation_backup="$HOME/.local/share/overmind/activation-backups/$candidate_version"
node "$stable_candidate/runtime/candidate.mjs" verify "$stable_candidate"
test -e "$active_codex" || test -L "$active_codex"
mkdir -p -m 700 "$(dirname "$activation_backup")"
mkdir -m 700 "$activation_backup"
cp -a -- "$active_codex" "$activation_backup/codex"
test ! -e "$active_codex.overmind-next" && test ! -L "$active_codex.overmind-next"
ln -s -- "$stable_candidate/overmind.mjs" "$active_codex.overmind-next"
mv -Tf -- "$active_codex.overmind-next" "$active_codex"
hash -r
```

The backup preserves the original file or the symlink text. To roll back that
activation, copy the backup to a new neighboring path and atomically restore it:

```bash
set -euo pipefail
test ! -e "$active_codex.overmind-rollback" && test ! -L "$active_codex.overmind-rollback"
cp -a -- "$activation_backup/codex" "$active_codex.overmind-rollback"
mv -Tf -- "$active_codex.overmind-rollback" "$active_codex"
hash -r
```

Stop candidate sessions before switching or rolling back. These commands leave
the active Codex configuration and credentials untouched; they do not migrate
state or alter the system package manager's installation.
