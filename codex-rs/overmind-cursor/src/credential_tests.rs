use pretty_assertions::assert_eq;

use super::*;

#[test]
fn env_value_wins_over_the_secrets_file() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        load_cursor_api_key(codex_home.path(), Some("  from_env  ".to_string())).expect("key"),
        "from_env"
    );
}

#[test]
fn reads_the_secrets_file_when_env_is_unset_or_blank() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(codex_home.path().join("secrets")).expect("secrets dir");
    std::fs::write(
        codex_home.path().join(SECRETS_FILE),
        "# Cursor SDK key\nCURSOR_API_KEY=from_file\n",
    )
    .expect("write");
    assert_eq!(
        [None, Some("   ".to_string())]
            .map(|env| load_cursor_api_key(codex_home.path(), env).expect("key")),
        ["from_file".to_string(), "from_file".to_string()]
    );
}

#[test]
fn parses_common_env_file_shapes() {
    assert_eq!(
        [
            "CURSOR_API_KEY=plain",
            "export CURSOR_API_KEY=\"quoted\"",
            "CURSOR_API_KEY = 'single'",
            "# CURSOR_API_KEY=commented\nCURSOR_API_KEY=first\nCURSOR_API_KEY=last",
            "OTHER=1",
            "CURSOR_API_KEY=",
        ]
        .map(parse_env_file),
        [
            Some("plain".to_string()),
            Some("quoted".to_string()),
            Some("single".to_string()),
            Some("last".to_string()),
            None,
            None,
        ]
    );
}

#[test]
fn missing_file_error_names_the_file() {
    let codex_home = tempfile::tempdir().expect("tempdir");
    let err = load_cursor_api_key(codex_home.path(), /*env_value*/ None).expect_err("no key");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(err.to_string().contains("secrets/cursor.env"));
}
