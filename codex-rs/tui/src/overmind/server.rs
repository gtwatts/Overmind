//! Overmind always serves sessions from its own embedded app server.
//!
//! The shared background daemon under `CODEX_HOME` belongs to the stock Codex install: it runs
//! the stock binary, which lacks Overmind's providers and features, and Overmind must never adopt
//! or start it. Every provider therefore gets the embedded server by default. An explicit
//! `--remote` endpoint is still honored because the user chose it. Setting
//! `OVERMIND_SHARED_DAEMON=allow` restores the upstream discovery and auto-start behavior; it
//! exists for the upstream daemon test suites, not for everyday use.

use std::ffi::OsStr;

/// Reason recorded as the daemon exclusion when Overmind keeps a launch embedded.
pub(crate) const EMBEDDED_ONLY_REASON: &str = "Overmind's built-in server";

/// Opt-out variable that lets the upstream shared-daemon behavior run.
pub(crate) const SHARED_DAEMON_ENV: &str = "OVERMIND_SHARED_DAEMON";

/// Whether this launch may discover, attach to or start the shared background daemon.
pub(crate) fn shared_daemon_allowed() -> bool {
    shared_daemon_allowed_by(std::env::var_os(SHARED_DAEMON_ENV).as_deref())
}

fn shared_daemon_allowed_by(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("allow"))
}

/// Combines the upstream daemon exclusion with Overmind's embedded-only policy. An upstream
/// reason wins so existing messages (for example `--no-daemon`) stay accurate.
pub(crate) fn daemon_exclusion(upstream: Option<&'static str>) -> Option<&'static str> {
    daemon_exclusion_with(upstream, shared_daemon_allowed())
}

fn daemon_exclusion_with(
    upstream: Option<&'static str>,
    shared_allowed: bool,
) -> Option<&'static str> {
    upstream.or((!shared_allowed).then_some(EMBEDDED_ONLY_REASON))
}

/// Whether an exclusion is worth a startup warning. Overmind's own policy is the normal mode, so
/// it is not announced as a fallback.
pub(crate) fn exclusion_needs_warning(reason: &str) -> bool {
    reason != EMBEDDED_ONLY_REASON
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
