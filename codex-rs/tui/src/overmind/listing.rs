//! The `/commands` listing: every custom command, where it came from, how its skills resolve,
//! and the files that were skipped.

use std::path::Path;

use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::custom_commands::CustomCommandDiscovery;
use super::custom_commands::CustomCommandEnv;
use super::custom_commands::CustomCommandSource;
use super::skill_refs::AvailableSkill;
use super::skill_refs::SkillRef;
use super::skill_refs::resolve_skill_refs;

/// Render the listing as transcript lines.
pub(crate) fn render_command_listing(
    discovery: &CustomCommandDiscovery,
    env: &CustomCommandEnv,
    available_skills: Option<&[AvailableSkill]>,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
        "/commands".magenta(),
        " · Overmind custom commands".bold(),
    ])];
    if discovery.commands.is_empty() {
        lines.push(Line::from("  (none)".dim()));
    }
    let name_width = discovery
        .commands
        .iter()
        .map(|command| command.name.len() + 1)
        .max()
        .unwrap_or(0);
    for command in &discovery.commands {
        let name = format!("/{}", command.name);
        lines.push(Line::from(vec![
            "  ".into(),
            Span::from(format!("{name:<name_width$}")).cyan(),
            "  ".into(),
            Span::from(command.popup_description()),
        ]));
        let mut details = vec![source_label(&command.source, env)];
        if let Some(skills) = skills_label(&command.skills, available_skills) {
            details.push(skills);
        }
        let indent = " ".repeat(name_width + 4);
        lines.push(Line::from(
            Span::from(format!("{indent}{}", details.join(" · "))).dim(),
        ));
    }
    if !discovery.warnings.is_empty() {
        let attention = crate::style::status_style(crate::style::StatusTone::Attention);
        lines.push(Line::from(""));
        lines.push(Line::from("Skipped".bold()));
        for warning in &discovery.warnings {
            lines.push(Line::from(vec![
                "  ⚠ ".set_style(attention),
                Span::from(warning.clone()),
            ]));
        }
    }
    lines.push(Line::from(""));
    let mut locations = vec![display_path(&env.user_commands_dir, env)];
    locations.extend(
        env.project_commands_dirs
            .iter()
            .map(|dir| display_path(dir, env)),
    );
    lines.push(Line::from(
        Span::from(format!(
            "Add a command by saving <name>.md in {}. Built-in commands win name clashes; edits apply the next time the / popup opens.",
            locations.join(" or ")
        ))
        .dim(),
    ));
    lines
}

fn source_label(source: &CustomCommandSource, env: &CustomCommandEnv) -> String {
    match source {
        CustomCommandSource::Bundled => "bundled with Overmind".to_string(),
        CustomCommandSource::User(path) => display_path(path, env),
        CustomCommandSource::Project(path) => format!("project: {}", display_path(path, env)),
    }
}

fn skills_label(skills: &[String], available_skills: Option<&[AvailableSkill]>) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let parts: Vec<String> = resolve_skill_refs(skills, available_skills)
        .iter()
        .map(|skill_ref| match skill_ref {
            SkillRef::Resolved { requested, skill } if requested == &skill.name => {
                skill.name.clone()
            }
            SkillRef::Resolved { requested, skill } => format!("{requested} → {}", skill.name),
            SkillRef::Ambiguous {
                requested,
                candidates,
            } => format!("{requested} (ambiguous: {})", candidates.join(", ")),
            SkillRef::Missing { requested } => format!("{requested} (not installed)"),
            SkillRef::Unverified { requested } => format!("{requested} (unchecked)"),
        })
        .collect();
    Some(format!("skills: {}", parts.join(", ")))
}

fn display_path(path: &Path, env: &CustomCommandEnv) -> String {
    if let Some(home) = &env.home
        && let Ok(rest) = path.strip_prefix(home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;
