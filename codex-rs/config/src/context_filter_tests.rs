use super::*;
use pretty_assertions::assert_eq;

#[test]
fn disabled_by_default() {
    let toml: ContextFilterToml = toml::from_str("command = [\"node\", \"f.mjs\"]").unwrap();
    assert_eq!(toml.resolve(), None);
}

#[test]
fn enabled_requires_command() {
    let toml: ContextFilterToml = toml::from_str("enabled = true").unwrap();
    assert_eq!(toml.resolve(), None);
    let toml: ContextFilterToml = toml::from_str("enabled = true\ncommand = [\" \"]").unwrap();
    assert_eq!(toml.resolve(), None);
}

#[test]
fn resolves_defaults() {
    let toml: ContextFilterToml =
        toml::from_str("enabled = true\ncommand = [\"node\", \"f.mjs\"]\ntimeout_ms = 900")
            .unwrap();
    assert_eq!(
        toml.resolve(),
        Some(ContextFilterConfig {
            command: vec!["node".to_string(), "f.mjs".to_string()],
            timeout: Duration::from_millis(900),
            filter_skills: true,
            filter_tool_outputs: true,
            min_tool_output_bytes: DEFAULT_CONTEXT_FILTER_MIN_TOOL_OUTPUT_BYTES,
        })
    );
}
