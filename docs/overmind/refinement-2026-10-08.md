# Overmind refinement candidate — October 8, 2026

The second pass improves selector recovery, Cursor usage accounting and local
candidate packaging. The installed Codex launcher, configuration, authentication
and PATH were not replaced. This candidate is intended for Gordon's next trial.

## Changes

- A repaired or rotated selector key can recover within the same turn. Concurrent
  preparations share one request; cancelled preparation does not automatically
  repeat possibly billed work. Steering cancels obsolete selection. Configuration
  loading is bounded, special files are rejected, and queued batches respect the
  whole-attempt deadline. Existing permission and exposure rules remain in force.
- Cursor catalog replacement drains the existing SDK consumer for at most 500 ms
  before closing its agent. Available exact counters are tracked by SDK run ID in
  optional `lineage_usage`, separately from context estimates and per-run ledger
  receipts. Duplicate retries cannot add cumulative snapshots twice. Authentication
  and model identity are checked before retirement; a new user turn resets lineage.
- A dependency-free packager creates private, versioned, relocatable development
  bundles with the CLI, code-mode host, Linux sandbox, helper and installed production
  dependency closure. Full verification checks every payload hash. Launch preflight
  checks structure, permissions, companions and Node compatibility. Explicit
  `--no-daemon` works without duplication. Default helper build/typecheck commands
  now match this repository's server-only layout.
- The coding harness checks a known failing Python project, unchanged tests, a
  successful unittest execution and a fresh MCP dispatch fact. Independent checks
  read current source with a unique bytecode-cache prefix. Recorder regressions
  cover test-first Python assertions, native shell quoting, zero-test runs and
  masked failures without retaining command text or test output in measurements.

## Verification

| Check | Result |
| --- | --- |
| Selector regressions | 28 passed |
| Native tool planning and permission regressions | 60 passed |
| Overmind TUI unit tests | 80 passed |
| Cursor helper tests | 324 passed across 44 files |
| Harness tests | 24 passed |
| Packaging tests, including relocation and fake activation/rollback | 15 passed |
| Actual CLI offline selection/fallback matrix | 14/14 passed |
| Actual CLI session lifecycle | 8/8 passed: six resumes, two progress-time interruptions and successful subsequent recovery |
| Final packaged launcher offline checks | 4/4 passed across native Direct and CodeModeOnly |

Helper default typecheck/build, Rust formatting and patch whitespace checks passed.
The CLI and both Rust companions were built offline using the existing matching
V8 archive and binding. An actual 120×36 terminal check showed reported MCP
progress, the approval gate, three completed stages, verified artifact rows,
fixed completed clocks and a visible composer. Its artifacts were precreated
synthetic evidence; this is a display check, not a claim that the model produced
those artifacts.

Live jobs used the actual packaged CLI and helper, isolated homes/projects,
`gpt-6.1-sol` in native CodeModeOnly and `composer-2.5` in native Direct mode.
No tool-mode override was applied. Each job was bounded to eight Responses
requests and 120 seconds, and each run stopped on its first failed check.
Measurement used HTTP fallback, so WebSocket performance was not measured.

| Live scenario | Plain | Decisions |
| --- | --- | --- |
| OpenAI fresh MCP fact | Passed | Passed |
| Cursor fresh MCP fact | Passed | Passed |
| OpenAI real Python repair, tests and fresh MCP fact | Passed | Passed |
| Cursor real Python repair, tests and fresh MCP fact | Passed | Passed |

Two earlier Cursor coding jobs produced correct source, passed independent tests,
and reported `Ran 2 tests` / `OK` with exit zero. Their command-format recorder
rejected valid evidence. The original failures were retained, the native quoting
problem was reproduced locally with pinned Rust shlex, and regressions were
added before the recorder fix. The final acceptance checks are recorded separately.
Ten paid jobs ran in total: eight accepted scenarios and those two preserved
recorder failures. There were no additional inference runs after acceptance.

The packaged CLI/helper payloads are identical by SHA-256 to those used for the
four fact checks; 1,357 runtime files were compared. The later launcher correction
changed packaging code and the launcher only. No general speed, dollar-cost or
threshold-tuning claim follows from this small sample. Selector defaults remain
two seconds, eight definitions and probability cutoff 0.55.

## Candidate identity and limits

- Candidate: `2026.10.08-refinement-839801f89`.
- Runtime source commit: `839801f899bb4ebef68592d87771521e485f2107`.
- Profile: Linux x64 **debug development build**, reporting `codex-cli 0.0.0`.
- Host Node used: 22.22.0; minimum required: 22.19.0. Node is not bundled.
- Pinned Cursor SDK: 1.0.30.
- Payload: 1,359 files, 1,519,546,820 bytes, with private directory/file modes.
- Manifest SHA-256: `13ff922447b6ced7d01ec373aa0a6115acd1844fa80f5adf9be0979c7e89f0a3`.
- CLI SHA-256: `e8fa95b96a0144a7ea8c2f2969643d7df88175532892937ec87dc1fc8e670c86`.

The cached V8 archive's distribution origin remains independently unauthenticated.
Checksums establish local integrity, not signed release provenance. Debug binaries
retain compiler/debug information. This is a private development bundle rather
than an optimized published release.

One live cancelled Cursor discovery run supplied no SDK usage at all: its store
contained `usage:null`, and only the replacement emitted usage. Retirement finished
well inside the 500 ms bound. Those aggregate tokens remain unknown, with
`complete:false` and `missing_runs:1`; the helper cannot reconstruct unreported
provider counters. Full Cursor cost comparisons remain unavailable for such runs.
Context occupancy remains an estimate, separate from exact reported usage.

These checks cover bounded fixtures and small coding tasks. Longer real-project
use, the full personal plugin setup and final replacement of the installed Codex
remain the next user trial. See [candidate installation and rollback](candidate-installation.md).
Private evidence is retained under `.codex/specs/refinement-pass/`; no credentials
or private SDK state were added to Git.
