use pretty_assertions::assert_eq;

use super::*;

#[test]
fn installed_helper_under_codex_home_is_preferred_over_the_source_tree() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    let installed = codex_home.path().join("overmind").join("cursor-helper");
    std::fs::create_dir_all(&installed).expect("helper dir");
    std::fs::write(installed.join(ENTRY), "").expect("entry");
    std::fs::write(installed.join("package.json"), "{}").expect("package.json");

    if std::env::var_os(HELPER_DIR_ENV).is_none() {
        assert_eq!(
            resolve_helper_dir(codex_home.path()).expect("dir"),
            installed
        );
    }
}

#[test]
fn source_tree_helper_is_found_without_an_installed_copy() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    if std::env::var_os(HELPER_DIR_ENV).is_none() {
        assert_eq!(
            resolve_helper_dir(codex_home.path()).expect("dir"),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("helper")
        );
    }
}

#[test]
fn helper_env_binds_loopback_with_byok_and_codex_compat() {
    let env: std::collections::HashMap<_, _> =
        helper_env(4242, Path::new("/state")).into_iter().collect();
    assert_eq!(
        (
            env.get("HOST").map(String::as_str),
            env.get("PORT").map(String::as_str),
            env.get("AUTH_MODE").map(String::as_str),
            env.get("STATE_DIR").map(String::as_str),
            env.get("OVERMIND_CODEX_COMPAT").map(String::as_str),
            env.contains_key("CURSOR_API_KEY"),
        ),
        (
            Some("127.0.0.1"),
            Some("4242"),
            Some("byok"),
            Some("/state"),
            Some("1"),
            false,
        )
    );
}
