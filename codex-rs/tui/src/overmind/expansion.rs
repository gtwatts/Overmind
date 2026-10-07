//! Expansion of `/name args` into the user turn for a custom command.

use std::path::Path;
use std::path::PathBuf;

use super::custom_commands::CustomCommand;
use super::custom_commands::CustomCommandEnv;
use super::custom_commands::PIPELINE_DEFINITION_FILE;
use super::custom_commands::PIPELINE_GUIDE_FILE;
use super::skill_refs::AvailableSkill;
use super::skill_refs::SkillRef;
use super::skill_refs::resolve_skill_refs;

/// The expanded user turn plus the skills to attach to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpandedCommand {
    pub(crate) text: String,
    /// Skills resolved from the command's `skills:` list, in order, to attach by path.
    pub(crate) skills: Vec<AvailableSkill>,
}

/// Build the user turn for `/name args`. `available_skills` is the loaded skills list, or `None`
/// while it has not loaded yet.
pub(crate) fn expand_custom_command(
    command: &CustomCommand,
    args: &str,
    env: &CustomCommandEnv,
    available_skills: Option<&[AvailableSkill]>,
) -> ExpandedCommand {
    let args = args.trim();
    let skill_refs = resolve_skill_refs(&command.skills, available_skills);
    let (mut text, used_args) = substitute_arguments(&command.body, args);
    if !used_args && !args.is_empty() {
        text.push_str("\n\n## Request\n\n");
        text.push_str(args);
    }
    let context = render_context(command, &skill_refs, env);
    if !context.is_empty() {
        text.push_str("\n\n");
        text.push_str(&context);
    }
    let skills = skill_refs
        .into_iter()
        .filter_map(|skill_ref| match skill_ref {
            SkillRef::Resolved { skill, .. } => Some(skill),
            SkillRef::Ambiguous { .. } | SkillRef::Missing { .. } | SkillRef::Unverified { .. } => {
                None
            }
        })
        .collect();
    ExpandedCommand { text, skills }
}

/// Replace `$ARGUMENTS`, `$1`..`$9` and `$$`. Returns whether any argument placeholder was used.
fn substitute_arguments(body: &str, args: &str) -> (String, bool) {
    let positional: Vec<&str> = args.split_whitespace().collect();
    let mut out = String::with_capacity(body.len() + args.len());
    let mut used_args = false;
    let mut rest = body;
    while let Some(idx) = rest.find('$') {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + 1..];
        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            out.push_str(args);
            used_args = true;
            rest = tail;
        } else if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
        } else if let Some(index) = after
            .chars()
            .next()
            .and_then(|ch| ch.to_digit(10))
            .filter(|digit| *digit >= 1)
        {
            out.push_str(positional.get(index as usize - 1).copied().unwrap_or(""));
            used_args = true;
            rest = &after[1..];
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    (out, used_args)
}

fn resolve_user_path(raw: &str, env: &CustomCommandEnv) -> PathBuf {
    if let Some(home) = &env.home {
        if raw == "~" {
            return home.clone();
        }
        if let Some(rest) = raw.strip_prefix("~/") {
            return home.join(rest);
        }
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env.cwd.join(path)
    }
}

fn resolve_pipeline_dir(entry: &str, env: &CustomCommandEnv) -> Option<PathBuf> {
    if entry.contains('/') || entry.starts_with('~') {
        let path = resolve_user_path(entry, env);
        return path.is_dir().then_some(path);
    }
    env.pipeline_dirs
        .iter()
        .map(|dir| dir.join(entry))
        .find(|dir| {
            dir.join(PIPELINE_DEFINITION_FILE).is_file() || dir.join(PIPELINE_GUIDE_FILE).is_file()
        })
}

fn render_context(
    command: &CustomCommand,
    skill_refs: &[SkillRef],
    env: &CustomCommandEnv,
) -> String {
    let mut lines = render_skill_lines(skill_refs);
    if !command.files.is_empty() {
        lines.push("- Read these files before starting:".to_string());
        for raw in &command.files {
            let path = resolve_user_path(raw, env);
            if path.exists() {
                lines.push(format!("  - {}", path.display()));
            } else {
                lines.push(format!("  - {} (not found; skip it)", path.display()));
            }
        }
    }
    if !command.pipelines.is_empty() {
        lines.push(format!(
            "- Use these pipelines as guidance ({PIPELINE_DEFINITION_FILE} holds the stages and dependencies, {PIPELINE_GUIDE_FILE} the guide):"
        ));
        for entry in &command.pipelines {
            match resolve_pipeline_dir(entry, env) {
                Some(dir) => {
                    let files: Vec<String> = [PIPELINE_DEFINITION_FILE, PIPELINE_GUIDE_FILE]
                        .iter()
                        .map(|file| dir.join(file))
                        .filter(|path| path.is_file())
                        .map(|path| path.display().to_string())
                        .collect();
                    lines.push(format!("  - {entry}: {}", files.join(", ")));
                }
                None => lines.push(format!("  - {entry} (pipeline not found; skip it)")),
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "Context for /{} (loaded by Overmind):\n{}",
        command.name,
        lines.join("\n")
    )
}

/// Skills that resolved are written with their full name (core also matches that exact name in
/// text); the structured attachment is what actually loads them. Entries that could not be
/// checked keep the plain `$name` form.
fn render_skill_lines(skill_refs: &[SkillRef]) -> Vec<String> {
    let mut mentions = Vec::new();
    let mut missing = Vec::new();
    let mut ambiguous = Vec::new();
    for skill_ref in skill_refs {
        match skill_ref {
            SkillRef::Resolved { skill, .. } => mentions.push(format!("${}", skill.name)),
            SkillRef::Unverified { requested } => mentions.push(format!("${requested}")),
            SkillRef::Missing { requested } => missing.push(requested.clone()),
            SkillRef::Ambiguous {
                requested,
                candidates,
            } => ambiguous.push(format!("{requested} ({})", candidates.join(", "))),
        }
    }
    let mut lines = Vec::new();
    if !mentions.is_empty() {
        lines.push(format!("- Use these skills: {}", mentions.join(" ")));
    }
    if !missing.is_empty() {
        lines.push(format!(
            "- These skills are not installed or are disabled; skip them: {}",
            missing.join(", ")
        ));
    }
    if !ambiguous.is_empty() {
        lines.push(format!(
            "- These skill names are ambiguous and were not loaded: {}",
            ambiguous.join("; ")
        ));
    }
    lines
}

#[cfg(test)]
#[path = "expansion_tests.rs"]
mod tests;
