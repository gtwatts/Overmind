use super::*;
use crate::AppServerTarget;
use crate::app_server_target_for_launch;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

fn default_socket() -> AbsolutePathBuf {
    AbsolutePathBuf::relative_to_current_dir("codex.sock").expect("absolute socket path")
}

fn target_for(
    exclusion: Option<&'static str>,
    remote: Option<RemoteAppServerEndpoint>,
) -> AppServerTarget {
    app_server_target_for_launch(
        remote,
        Some(default_socket()),
        /*can_reuse_implicit_local_daemon*/ exclusion.is_none(),
        /*workload_identity_selected*/ false,
        /*exec_server_url*/ None,
    )
    .expect("launch target")
}

#[test]
fn embedded_only_is_the_default() {
    assert!(!shared_daemon_allowed_by(None));
    assert!(!shared_daemon_allowed_by(Some(OsStr::new(""))));
    assert!(!shared_daemon_allowed_by(Some(OsStr::new("1"))));
    assert!(shared_daemon_allowed_by(Some(OsStr::new("allow"))));
    assert!(shared_daemon_allowed_by(Some(OsStr::new("ALLOW"))));
}

#[test]
fn exclusion_applies_to_every_launch_without_an_upstream_reason() {
    assert_eq!(
        daemon_exclusion_with(/*upstream*/ None, /*shared_allowed*/ false),
        Some(EMBEDDED_ONLY_REASON)
    );
    assert_eq!(
        daemon_exclusion_with(Some("--no-daemon"), /*shared_allowed*/ false),
        Some("--no-daemon")
    );
    assert_eq!(
        daemon_exclusion_with(/*upstream*/ None, /*shared_allowed*/ true),
        None
    );
}

#[test]
fn a_running_shared_daemon_is_never_adopted() {
    let exclusion = daemon_exclusion_with(/*upstream*/ None, /*shared_allowed*/ false);
    assert_eq!(
        target_for(exclusion, /*remote*/ None),
        AppServerTarget::Embedded
    );
}

#[test]
fn the_opt_out_restores_upstream_discovery() {
    let exclusion = daemon_exclusion_with(/*upstream*/ None, /*shared_allowed*/ true);
    assert_eq!(
        target_for(exclusion, /*remote*/ None),
        AppServerTarget::LocalDaemon {
            allow_embedded_fallback: true,
            endpoint: RemoteAppServerEndpoint::UnixSocket {
                socket_path: default_socket(),
            },
        }
    );
}

#[test]
fn an_explicit_remote_endpoint_is_still_honored() {
    let endpoint = RemoteAppServerEndpoint::UnixSocket {
        socket_path: default_socket(),
    };
    let exclusion = daemon_exclusion_with(/*upstream*/ None, /*shared_allowed*/ false);
    assert_eq!(
        target_for(exclusion, Some(endpoint.clone())),
        AppServerTarget::Remote { endpoint }
    );
}

#[test]
fn only_upstream_reasons_are_announced() {
    assert!(!exclusion_needs_warning(EMBEDDED_ONLY_REASON));
    assert!(exclusion_needs_warning("--profile"));
}
