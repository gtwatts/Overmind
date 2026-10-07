use std::io;
use std::path::Path;

pub(crate) const CURSOR_API_KEY_ENV: &str = "CURSOR_API_KEY";
const SECRETS_FILE: &str = "secrets/cursor.env";

/// Returns the Cursor API key from `CURSOR_API_KEY`, else from
/// `$CODEX_HOME/secrets/cursor.env` (a `CURSOR_API_KEY=...` line). The key is
/// never logged; errors only name the file.
pub(crate) fn load_cursor_api_key(
    codex_home: &Path,
    env_value: Option<String>,
) -> io::Result<String> {
    if let Some(value) = env_value.map(|value| value.trim().to_string())
        && !value.is_empty()
    {
        return Ok(value);
    }
    let path = codex_home.join(SECRETS_FILE);
    let contents = std::fs::read_to_string(&path).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "Cursor models need a Cursor API key: set {CURSOR_API_KEY_ENV} or add `{CURSOR_API_KEY_ENV}=...` to {} (mode 600). ({err})",
                path.display()
            ),
        )
    })?;
    parse_env_file(&contents).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} has no {CURSOR_API_KEY_ENV}=... line", path.display()),
        )
    })
}

fn parse_env_file(contents: &str) -> Option<String> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.strip_prefix("export ").unwrap_or(line))
        .filter_map(|line| {
            line.strip_prefix(CURSOR_API_KEY_ENV)?
                .trim_start()
                .strip_prefix('=')
        })
        .map(|value| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        })
        .rfind(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "credential_tests.rs"]
mod tests;
