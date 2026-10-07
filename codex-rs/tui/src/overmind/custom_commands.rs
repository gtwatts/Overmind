//! User-defined slash commands.
//!
//! A custom command is a markdown file whose file stem is the command name, so
//! `~/.codex/commands/video.md` becomes `/video`. Commands are discovered from three layers,
//! lowest precedence first; a later layer replaces an earlier command with the same name:
//!
//! 1. bundled defaults shipped with Overmind (`tui/assets/overmind/commands/*.md`);
//! 2. the user directory `$CODEX_HOME/commands/*.md`;
//! 3. `<git root>/.codex/commands/*.md`, then `<cwd>/.codex/commands/*.md`, only when the active
//!    project is trusted.
//!
//! Built-in slash commands always win a name clash. Malformed files are skipped with a warning
//! and never abort discovery.
//!
//! The optional YAML frontmatter supports:
//!
//! ```yaml
//! description: shown next to the command in the slash popup
//! argument-hint: "<brief>"
//! skills: [photocraft, blender]        # attached as skills (see `skill_refs`)
//! files: [~/notes/house-style.md]      # listed for the agent to read first
//! pipelines: [high-end-whiteboard]     # resolved to pipeline.yaml + PIPELINE.md
//! ```
//!
//! Expansion into the user turn lives in `expansion`. The body becomes the user turn. It may use `$ARGUMENTS` (all text after the command name),
//! `$1`..`$9` (whitespace-separated words) and `$$` (a literal `$`). When the body uses none of
//! the argument placeholders, the arguments are appended under a `## Request` heading.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use serde::Deserialize;

use crate::slash_command::SlashCommand;

const COMMAND_FILE_EXTENSION: &str = "md";
const MAX_COMMAND_FILE_BYTES: u64 = 64 * 1024;
const MAX_COMMAND_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 120;
const DEFAULT_DESCRIPTION: &str = "custom command";
pub(crate) const PIPELINE_DEFINITION_FILE: &str = "pipeline.yaml";
pub(crate) const PIPELINE_GUIDE_FILE: &str = "PIPELINE.md";

/// Commands compiled into the binary. User and project files with the same name override them.
const BUNDLED_COMMANDS: &[(&str, &str)] = &[
    (
        "examples",
        include_str!("../../assets/overmind/commands/examples.md"),
    ),
    (
        "photocraft",
        include_str!("../../assets/overmind/commands/photocraft.md"),
    ),
    (
        "video",
        include_str!("../../assets/overmind/commands/video.md"),
    ),
    (
        "whiteboard",
        include_str!("../../assets/overmind/commands/whiteboard.md"),
    ),
];

/// Where a custom command was loaded from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CustomCommandSource {
    Bundled,
    User(PathBuf),
    Project(PathBuf),
}

/// A parsed custom slash command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) argument_hint: Option<String>,
    pub(crate) skills: Vec<String>,
    pub(crate) files: Vec<String>,
    pub(crate) pipelines: Vec<String>,
    pub(crate) body: String,
    pub(crate) source: CustomCommandSource,
}

impl CustomCommand {
    /// Text shown in the slash popup next to `/name`.
    pub(crate) fn popup_description(&self) -> String {
        match &self.argument_hint {
            Some(hint) => format!("{} {hint}", self.description),
            None => self.description.clone(),
        }
    }
}

/// Result of scanning every command layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CustomCommandDiscovery {
    /// Commands sorted by name, after precedence and built-in filtering.
    pub(crate) commands: Vec<Arc<CustomCommand>>,
    /// Human-readable problems with individual files.
    pub(crate) warnings: Vec<String>,
}

/// Whether project-local commands and pipelines may be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectTrust {
    Trusted,
    NotTrusted,
}

/// Filesystem locations used for discovery and expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomCommandEnv {
    /// `$CODEX_HOME/commands`.
    pub(crate) user_commands_dir: PathBuf,
    /// Project command directories, lowest precedence first.
    pub(crate) project_commands_dirs: Vec<PathBuf>,
    /// Pipeline search directories, highest precedence first.
    pub(crate) pipeline_dirs: Vec<PathBuf>,
    pub(crate) cwd: PathBuf,
    pub(crate) home: Option<PathBuf>,
}

impl CustomCommandEnv {
    pub(crate) fn resolve(codex_home: &Path, cwd: &Path, trust: ProjectTrust) -> Self {
        let user_commands_dir = codex_home.join("commands");
        let mut project_roots = Vec::new();
        if trust == ProjectTrust::Trusted {
            if let Some(git_root) = cwd.ancestors().find(|dir| dir.join(".git").exists()) {
                project_roots.push(git_root.to_path_buf());
            }
            if !project_roots.iter().any(|root| root == cwd) {
                project_roots.push(cwd.to_path_buf());
            }
        }
        let project_commands_dirs = project_roots
            .iter()
            .map(|root| root.join(".codex").join("commands"))
            .filter(|dir| dir != &user_commands_dir)
            .collect();
        let mut pipeline_dirs: Vec<PathBuf> = project_roots
            .iter()
            .rev()
            .map(|root| root.join(".codex").join("pipelines"))
            .collect();
        pipeline_dirs.push(codex_home.join("pipelines"));
        pipeline_dirs.extend(plugin_pipeline_dirs(codex_home));
        pipeline_dirs.dedup();
        Self {
            user_commands_dir,
            project_commands_dirs,
            pipeline_dirs,
            cwd: cwd.to_path_buf(),
            home: dirs::home_dir(),
        }
    }
}

/// Pipelines shipped by locally installed marketplace plugins, for example
/// `$CODEX_HOME/local-marketplaces/<marketplace>/plugins/<plugin>/pipelines`.
fn plugin_pipeline_dirs(codex_home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for marketplace in sorted_dir_entries(&codex_home.join("local-marketplaces")) {
        for plugin in sorted_dir_entries(&marketplace.join("plugins")) {
            let pipelines = plugin.join("pipelines");
            if pipelines.is_dir() {
                dirs.push(pipelines);
            }
        }
    }
    dirs
}

fn sorted_dir_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();
    paths
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Frontmatter {
    description: Option<String>,
    #[serde(alias = "argument_hint")]
    argument_hint: Option<String>,
    skills: Option<OneOrMany>,
    files: Option<OneOrMany>,
    pipelines: Option<OneOrMany>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

fn clean_list(value: Option<OneOrMany>) -> Vec<String> {
    let values = match value {
        None => Vec::new(),
        Some(OneOrMany::One(value)) => vec![value],
        Some(OneOrMany::Many(values)) => values,
    };
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn validate_command_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > MAX_COMMAND_NAME_LEN {
        return Err(format!(
            "command names must be 1-{MAX_COMMAND_NAME_LEN} characters long"
        ));
    }
    let valid_chars = name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
    let valid_start = name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit());
    if valid_chars && valid_start {
        Ok(())
    } else {
        Err(
            "command names may only use lowercase letters, digits, `-` and `_`, and must start with a letter or digit"
                .to_string(),
        )
    }
}

/// Split `contents` into `(frontmatter, body)`. Frontmatter is optional.
fn split_frontmatter(contents: &str) -> Result<(Option<&str>, &str), String> {
    let mut lines = contents.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok((None, contents));
    };
    if first.trim_end() != "---" {
        return Ok((None, contents));
    }
    let mut offset = first.len();
    for line in lines {
        if line.trim_end() == "---" {
            let frontmatter = &contents[first.len()..offset];
            let body = &contents[offset + line.len()..];
            return Ok((Some(frontmatter), body));
        }
        offset += line.len();
    }
    Err("frontmatter starts with `---` but has no closing `---` line".to_string())
}

fn fallback_description(body: &str) -> String {
    body.lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .unwrap_or(DEFAULT_DESCRIPTION)
        .to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

/// Parse one command file. `name` is the file stem.
pub(crate) fn parse_custom_command(
    name: &str,
    contents: &str,
    source: CustomCommandSource,
) -> Result<CustomCommand, String> {
    validate_command_name(name)?;
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
    let (frontmatter, body) = split_frontmatter(contents)?;
    let frontmatter = match frontmatter {
        Some(raw) => serde_yaml::from_str::<Option<Frontmatter>>(raw)
            .map_err(|err| format!("invalid frontmatter: {err}"))?
            .unwrap_or_default(),
        None => Frontmatter::default(),
    };
    let body = body.trim();
    if body.is_empty() {
        return Err("command body is empty".to_string());
    }
    let description = frontmatter
        .description
        .map(|description| description.trim().to_string())
        .filter(|description| !description.is_empty())
        .unwrap_or_else(|| fallback_description(body));
    let argument_hint = frontmatter
        .argument_hint
        .map(|hint| hint.trim().to_string())
        .filter(|hint| !hint.is_empty());
    let skills = clean_list(frontmatter.skills)
        .into_iter()
        .map(|skill| skill.trim_start_matches('$').to_string())
        .filter(|skill| !skill.is_empty())
        .collect();
    Ok(CustomCommand {
        name: name.to_string(),
        description: truncate_chars(&description, MAX_DESCRIPTION_CHARS),
        argument_hint,
        skills,
        files: clean_list(frontmatter.files),
        pipelines: clean_list(frontmatter.pipelines),
        body: body.to_string(),
        source,
    })
}

/// Whether `name` is reserved by a built-in slash command (including aliases).
pub(crate) fn is_builtin_command_name(name: &str) -> bool {
    SlashCommand::from_str(name).is_ok()
}

/// Scan bundled, user and project commands and apply precedence.
pub(crate) fn discover_custom_commands(env: &CustomCommandEnv) -> CustomCommandDiscovery {
    let mut by_name: BTreeMap<String, CustomCommand> = BTreeMap::new();
    let mut warnings = Vec::new();
    for (name, contents) in BUNDLED_COMMANDS {
        match parse_custom_command(name, contents, CustomCommandSource::Bundled) {
            Ok(command) => {
                by_name.insert(command.name.clone(), command);
            }
            Err(err) => warnings.push(format!("Skipped bundled command /{name}: {err}")),
        }
    }
    load_command_dir(
        &env.user_commands_dir,
        CustomCommandSource::User,
        &mut by_name,
        &mut warnings,
    );
    for dir in &env.project_commands_dirs {
        load_command_dir(
            dir,
            CustomCommandSource::Project,
            &mut by_name,
            &mut warnings,
        );
    }

    let mut commands = Vec::with_capacity(by_name.len());
    for command in by_name.into_values() {
        if is_builtin_command_name(&command.name) {
            warnings.push(format!(
                "Skipped custom command /{}: the name is used by a built-in command",
                command.name
            ));
        } else {
            commands.push(Arc::new(command));
        }
    }
    CustomCommandDiscovery { commands, warnings }
}

/// Cheap change detector for the user and project command directories: every `.md` entry with
/// its size and modification time. Discovery only needs to run again when this changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CommandDirsFingerprint(Vec<(PathBuf, Option<SystemTime>, u64)>);

pub(crate) fn command_dirs_fingerprint(env: &CustomCommandEnv) -> CommandDirsFingerprint {
    let mut entries = Vec::new();
    for dir in std::iter::once(&env.user_commands_dir).chain(&env.project_commands_dirs) {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        for path in read_dir.filter_map(Result::ok).map(|entry| entry.path()) {
            if path
                .extension()
                .is_none_or(|extension| extension != COMMAND_FILE_EXTENSION)
            {
                continue;
            }
            let metadata = fs::metadata(&path).ok();
            let modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok());
            let len = metadata.map_or(0, |metadata| metadata.len());
            entries.push((path, modified, len));
        }
    }
    entries.sort();
    CommandDirsFingerprint(entries)
}

fn load_command_dir(
    dir: &Path,
    source: fn(PathBuf) -> CustomCommandSource,
    by_name: &mut BTreeMap<String, CustomCommand>,
    warnings: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warnings.push(format!(
                "Could not read custom commands in {}: {err}",
                dir.display()
            ));
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == COMMAND_FILE_EXTENSION)
        })
        .collect();
    paths.sort();
    for path in paths {
        match load_command_file(&path, source) {
            Ok(Some(command)) => {
                by_name.insert(command.name.clone(), command);
            }
            Ok(None) => {}
            Err(err) => warnings.push(format!("Skipped custom command {}: {err}", path.display())),
        }
    }
}

fn load_command_file(
    path: &Path,
    source: fn(PathBuf) -> CustomCommandSource,
) -> Result<Option<CustomCommand>, String> {
    let metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_COMMAND_FILE_BYTES {
        return Err(format!(
            "file is larger than {} KiB",
            MAX_COMMAND_FILE_BYTES / 1024
        ));
    }
    let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Err("file name is not valid UTF-8".to_string());
    };
    let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
    parse_custom_command(name, &contents, source(path.to_path_buf())).map(Some)
}

#[cfg(test)]
#[path = "custom_commands_tests.rs"]
mod tests;
