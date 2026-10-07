//! Opt-in, per-request context filter (`[context_filter]` in `config.toml`).
//!
//! Right before each model request Codex can hand an external command a JSON summary of the
//! request: the latest user prompt, the skills listed in the `<skills_instructions>` catalog
//! and the older, larger tool outputs. The command answers with the skills to keep and the
//! tool outputs to elide. The decision is applied to the outgoing request only; the session
//! history is never modified, so the next request starts from the full history again.
//!
//! The filter fails open: a spawn failure, timeout, non-zero exit or malformed JSON sends the
//! request unfiltered and logs a warning. The latest tool output and every user message are
//! never offered to the filter, and skills the user mentions as `$name` are always kept.
//!
//! Protocol (version 1), stdin:
//! `{"version":1,"turn_id","model","cwd","prompt","skills":[{"name","description","locator","path"}],
//!   "tool_outputs":[{"id","tool","call","bytes","approx_tokens","current_turn","success","preview"}]}`
//! stdout: `{"keep_skills":["name",...] | null, "elide_tool_outputs":["id",...]}`.
//! A missing or `null` `keep_skills` keeps every skill.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use codex_config::ContextFilterConfig;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_OPEN_TAG;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::info;
use tracing::warn;

pub(crate) const CONTEXT_FILTER_PROTOCOL_VERSION: u32 = 1;
pub(crate) const ELIDED_PLACEHOLDER_PREFIX: &str = "[elided by context filter: ";
const PREVIEW_HEAD_CHARS: usize = 1_500;
const PREVIEW_TAIL_CHARS: usize = 400;
const CALL_PREVIEW_CHARS: usize = 300;
const STDERR_LOG_CHARS: usize = 400;
const SKILL_LOCATOR_KINDS: [&str; 4] = [
    "file",
    "executor package",
    "cloud package",
    "custom resource",
];

/// Request metadata that is not derived from the prompt items.
pub(crate) struct ContextFilterRequestMeta<'a> {
    pub(crate) turn_id: &'a str,
    pub(crate) model: &'a str,
    pub(crate) cwd: &'a str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ContextFilterRequest {
    version: u32,
    turn_id: String,
    model: String,
    cwd: String,
    prompt: String,
    skills: Vec<FilterSkill>,
    tool_outputs: Vec<FilterToolOutput>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct FilterSkill {
    name: String,
    description: String,
    locator: String,
    /// Absolute `SKILL.md` path when the locator is a (possibly aliased) file path.
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct FilterToolOutput {
    id: String,
    tool: Option<String>,
    call: Option<String>,
    bytes: usize,
    approx_tokens: usize,
    /// True when the output was produced after the latest user message.
    current_turn: bool,
    success: Option<bool>,
    preview: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub(crate) struct ContextFilterResponse {
    #[serde(default)]
    keep_skills: Option<Vec<String>>,
    #[serde(default)]
    elide_tool_outputs: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ContextFilterOutcome {
    pub(crate) skills_listed: usize,
    pub(crate) skills_hidden: usize,
    pub(crate) tool_outputs_offered: usize,
    pub(crate) tool_outputs_elided: usize,
    pub(crate) bytes_removed: usize,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ContextFilterError {
    #[error("failed to start `{program}`: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("exited with {status}; stderr: {stderr}")]
    Exit { status: String, stderr: String },
    #[error("invalid JSON on stdout: {0}")]
    BadJson(#[from] serde_json::Error),
}

/// Positions in the prompt that the filter is allowed to change.
#[derive(Debug, Default)]
struct FilterPlan {
    skill_names: HashSet<String>,
    mentioned_skills: HashSet<String>,
    /// call id -> (item index, approximate tokens)
    tool_outputs: HashMap<String, (usize, usize)>,
}

/// Runs the configured filter and applies its answer to `items` (this request only).
/// Never fails: on any error `items` is left untouched and a warning is logged.
pub(crate) async fn filter_prompt_input(
    config: &ContextFilterConfig,
    items: &mut [ResponseItem],
    meta: ContextFilterRequestMeta<'_>,
) -> Option<ContextFilterOutcome> {
    let started = Instant::now();
    let (request, plan) = build_request(config, items, &meta);
    if request.skills.is_empty() && request.tool_outputs.is_empty() {
        return None;
    }
    let response = match run_filter_command(config, &request).await {
        Ok(response) => response,
        Err(err) => {
            warn!(
                turn_id = meta.turn_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "context filter failed; sending request unfiltered: {err}"
            );
            return None;
        }
    };
    let outcome = apply_response(items, &plan, &response);
    info!(
        turn_id = meta.turn_id,
        elapsed_ms = started.elapsed().as_millis() as u64,
        skills_listed = outcome.skills_listed,
        skills_hidden = outcome.skills_hidden,
        tool_outputs_offered = outcome.tool_outputs_offered,
        tool_outputs_elided = outcome.tool_outputs_elided,
        bytes_removed = outcome.bytes_removed,
        "context filter applied"
    );
    Some(outcome)
}

fn build_request(
    config: &ContextFilterConfig,
    items: &[ResponseItem],
    meta: &ContextFilterRequestMeta<'_>,
) -> (ContextFilterRequest, FilterPlan) {
    let mut plan = FilterPlan::default();
    let last_user_index = items.iter().rposition(is_user_message);
    let prompt = last_user_index
        .and_then(
            |index| match crate::event_mapping::parse_turn_item(&items[index]) {
                Some(TurnItem::UserMessage(message)) => Some(message.message()),
                _ => None,
            },
        )
        .unwrap_or_default();

    let mut skills = Vec::new();
    if config.filter_skills {
        let mut seen = HashSet::new();
        for text in items.iter().filter_map(skills_catalog_text) {
            for skill in parse_skills_catalog(text) {
                if seen.insert(skill.name.clone()) {
                    skills.push(skill);
                }
            }
        }
        plan.skill_names = seen;
        plan.mentioned_skills = plan
            .skill_names
            .iter()
            .filter(|name| prompt.contains(&format!("${name}")))
            .cloned()
            .collect();
    }

    let mut tool_outputs = Vec::new();
    if config.filter_tool_outputs {
        let latest_output = items
            .iter()
            .rposition(|item| tool_output_parts(item).is_some());
        let calls = tool_calls_by_id(items);
        for (index, item) in items.iter().enumerate() {
            if Some(index) == latest_output {
                continue;
            }
            let Some((call_id, name, output)) = tool_output_parts(item) else {
                continue;
            };
            let Some(text) = output_text(output) else {
                continue;
            };
            if text.len() < config.min_tool_output_bytes
                || text.starts_with(ELIDED_PLACEHOLDER_PREFIX)
                || plan.tool_outputs.contains_key(call_id)
            {
                continue;
            }
            let approx_tokens = text.len().div_ceil(4);
            let call = calls.get(call_id);
            plan.tool_outputs
                .insert(call_id.to_string(), (index, approx_tokens));
            tool_outputs.push(FilterToolOutput {
                id: call_id.to_string(),
                tool: name
                    .map(str::to_string)
                    .or_else(|| call.map(|(tool, _)| tool.to_string())),
                call: call.map(|(_, args)| truncate_chars(args, CALL_PREVIEW_CHARS)),
                bytes: text.len(),
                approx_tokens,
                current_turn: last_user_index.is_some_and(|user| index > user),
                success: output.success,
                preview: preview(&text),
            });
        }
    }

    let request = ContextFilterRequest {
        version: CONTEXT_FILTER_PROTOCOL_VERSION,
        turn_id: meta.turn_id.to_string(),
        model: meta.model.to_string(),
        cwd: meta.cwd.to_string(),
        prompt,
        skills,
        tool_outputs,
    };
    (request, plan)
}

fn apply_response(
    items: &mut [ResponseItem],
    plan: &FilterPlan,
    response: &ContextFilterResponse,
) -> ContextFilterOutcome {
    let mut outcome = ContextFilterOutcome {
        skills_listed: plan.skill_names.len(),
        tool_outputs_offered: plan.tool_outputs.len(),
        ..Default::default()
    };

    if let Some(keep) = &response.keep_skills {
        let keep: HashSet<&str> = keep
            .iter()
            .map(String::as_str)
            .chain(plan.mentioned_skills.iter().map(String::as_str))
            .collect();
        let mut hidden = HashSet::new();
        for item in items.iter_mut() {
            let ResponseItem::Message { role, content, .. } = item else {
                continue;
            };
            if role != "developer" {
                continue;
            }
            for content_item in content.iter_mut() {
                let ContentItem::InputText { text } = content_item else {
                    continue;
                };
                if !text.trim_start().starts_with(SKILLS_INSTRUCTIONS_OPEN_TAG) {
                    continue;
                }
                if let Some((filtered, removed)) = filter_skills_catalog(text, &keep) {
                    outcome.bytes_removed += text.len().saturating_sub(filtered.len());
                    hidden.extend(removed);
                    *text = filtered;
                }
            }
        }
        outcome.skills_hidden = hidden.len();
    }

    for id in &response.elide_tool_outputs {
        let Some((index, approx_tokens)) = plan.tool_outputs.get(id) else {
            continue;
        };
        let Some(output) = items.get_mut(*index).and_then(tool_output_payload_mut) else {
            continue;
        };
        let before = output_text(output).map_or(0, |text| text.len());
        let placeholder = format!("{ELIDED_PLACEHOLDER_PREFIX}{approx_tokens} tokens]");
        outcome.bytes_removed += before.saturating_sub(placeholder.len());
        output.body = FunctionCallOutputBody::Text(placeholder);
        outcome.tool_outputs_elided += 1;
    }
    outcome
}

fn is_user_message(item: &ResponseItem) -> bool {
    matches!(
        crate::event_mapping::parse_turn_item(item),
        Some(TurnItem::UserMessage(_))
    )
}

fn skills_catalog_text(item: &ResponseItem) -> Option<&str> {
    let ResponseItem::Message { role, content, .. } = item else {
        return None;
    };
    if role != "developer" {
        return None;
    }
    content.iter().find_map(|content_item| match content_item {
        ContentItem::InputText { text }
            if text.trim_start().starts_with(SKILLS_INSTRUCTIONS_OPEN_TAG) =>
        {
            Some(text.as_str())
        }
        _ => None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogSection {
    Other,
    Roots,
    Skills,
}

fn catalog_section(line: &str, current: CatalogSection) -> CatalogSection {
    match line.trim_end() {
        "### Skill roots" => CatalogSection::Roots,
        "### Available skills" => CatalogSection::Skills,
        other if other.starts_with("### ") || other.starts_with("## ") => CatalogSection::Other,
        _ => current,
    }
}

struct SkillLine<'a> {
    name: &'a str,
    description: &'a str,
    kind: &'a str,
    locator: &'a str,
}

/// Parses `- name: description (kind: locator)` as rendered by the skills extension.
fn parse_skill_line(line: &str) -> Option<SkillLine<'_>> {
    let rest = line.strip_prefix("- ")?;
    let (name, rest) = rest.split_once(": ")?;
    let inner = rest.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    let (kind, locator) = inner[open + 1..].split_once(": ")?;
    if name.is_empty() || name.contains(' ') || !SKILL_LOCATOR_KINDS.contains(&kind) {
        return None;
    }
    Some(SkillLine {
        name,
        description: inner[..open].trim_end(),
        kind,
        locator,
    })
}

/// Parses ``- `alias` = `/abs/path` `` root lines.
fn parse_root_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("- `")?;
    let (alias, rest) = rest.split_once("` = `")?;
    Some((alias, rest.strip_suffix('`')?))
}

fn parse_skills_catalog(text: &str) -> Vec<FilterSkill> {
    let mut section = CatalogSection::Other;
    let mut roots = HashMap::new();
    let mut lines = Vec::new();
    for line in text.lines() {
        section = catalog_section(line, section);
        match section {
            CatalogSection::Roots => {
                if let Some((alias, path)) = parse_root_line(line) {
                    roots.insert(alias, path);
                }
            }
            CatalogSection::Skills => {
                if let Some(skill) = parse_skill_line(line) {
                    lines.push(skill);
                }
            }
            CatalogSection::Other => {}
        }
    }
    lines
        .into_iter()
        .map(|skill| {
            let path = (skill.kind == "file").then(|| {
                if skill.locator.starts_with('/') {
                    skill.locator.to_string()
                } else {
                    skill
                        .locator
                        .split_once('/')
                        .and_then(|(alias, rest)| {
                            roots
                                .get(alias)
                                .map(|root| format!("{}/{rest}", root.trim_end_matches('/')))
                        })
                        .unwrap_or_else(|| skill.locator.to_string())
                }
            });
            FilterSkill {
                name: skill.name.to_string(),
                description: skill.description.to_string(),
                locator: skill.locator.to_string(),
                path,
            }
        })
        .collect()
}

/// Drops catalog lines for skills outside `keep`. Returns `None` when nothing changes.
fn filter_skills_catalog(text: &str, keep: &HashSet<&str>) -> Option<(String, Vec<String>)> {
    let mut section = CatalogSection::Other;
    let mut out: Vec<&str> = Vec::new();
    let mut removed = Vec::new();
    let mut note_at = None;
    for line in text.split('\n') {
        let previous = section;
        section = catalog_section(line, section);
        if previous == CatalogSection::Skills && section != CatalogSection::Skills {
            note_at.get_or_insert(out.len());
        }
        if section == CatalogSection::Skills
            && let Some(skill) = parse_skill_line(line)
            && !keep.contains(skill.name)
        {
            removed.push(skill.name.to_string());
            continue;
        }
        out.push(line);
    }
    if removed.is_empty() {
        return None;
    }
    let note = format!(
        "- ({} more skills are hidden from this request by the context filter. Mention one as `$name` to load it.)",
        removed.len()
    );
    let mut lines: Vec<String> = out.into_iter().map(str::to_string).collect();
    let at = note_at.unwrap_or_else(|| {
        // The catalog ended inside the skills section: insert before the closing tag.
        lines
            .iter()
            .rposition(|line| line.trim_start().starts_with("</"))
            .unwrap_or(lines.len())
    });
    lines.insert(at, note);
    Some((lines.join("\n"), removed))
}

fn tool_output_parts(
    item: &ResponseItem,
) -> Option<(&str, Option<&str>, &FunctionCallOutputPayload)> {
    match item {
        ResponseItem::FunctionCallOutput {
            call_id: Some(call_id),
            name,
            output,
            ..
        } => Some((call_id.as_str(), name.as_deref(), output)),
        ResponseItem::CustomToolCallOutput {
            call_id,
            name,
            output,
            ..
        } => Some((call_id.as_str(), name.as_deref(), output)),
        _ => None,
    }
}

fn tool_output_payload_mut(item: &mut ResponseItem) -> Option<&mut FunctionCallOutputPayload> {
    match item {
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => Some(output),
        _ => None,
    }
}

fn tool_calls_by_id(items: &[ResponseItem]) -> HashMap<&str, (&str, &str)> {
    items
        .iter()
        .filter_map(|item| match item {
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => Some((call_id.as_str(), (name.as_str(), arguments.as_str()))),
            ResponseItem::CustomToolCall {
                name,
                input,
                call_id,
                ..
            } => Some((call_id.as_str(), (name.as_str(), input.as_str()))),
            _ => None,
        })
        .collect()
}

/// Text of a tool output. Content-item outputs (e.g. code mode) count when every item is
/// text; outputs that carry images, audio or encrypted content are never offered.
fn output_text(output: &FunctionCallOutputPayload) -> Option<Cow<'_, str>> {
    match &output.body {
        FunctionCallOutputBody::Text(text) => Some(Cow::Borrowed(text.as_str())),
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .map(|item| match item {
                FunctionCallOutputContentItem::InputText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|texts| Cow::Owned(texts.concat())),
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

fn preview(text: &str) -> String {
    let total = text.chars().count();
    if total <= PREVIEW_HEAD_CHARS + PREVIEW_TAIL_CHARS {
        return text.to_string();
    }
    let head_end = text
        .char_indices()
        .nth(PREVIEW_HEAD_CHARS)
        .map_or(text.len(), |(index, _)| index);
    let tail_start = text
        .char_indices()
        .nth(total - PREVIEW_TAIL_CHARS)
        .map_or(text.len(), |(index, _)| index);
    format!("{}\n…\n{}", &text[..head_end], &text[tail_start..])
}

async fn run_filter_command(
    config: &ContextFilterConfig,
    request: &ContextFilterRequest,
) -> Result<ContextFilterResponse, ContextFilterError> {
    let payload = serde_json::to_vec(request)?;
    let program = config.command[0].clone();
    let mut child = Command::new(&program)
        .args(&config.command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| ContextFilterError::Spawn { program, source })?;
    let stdin = child.stdin.take();
    let run = async move {
        let write = async move {
            if let Some(mut stdin) = stdin {
                // The filter may exit without reading everything; its answer still counts.
                let _ = stdin.write_all(&payload).await;
                let _ = stdin.shutdown().await;
            }
        };
        let (_, output) = tokio::join!(write, child.wait_with_output());
        output
    };
    let output = tokio::time::timeout(config.timeout, run)
        .await
        .map_err(|_| ContextFilterError::Timeout(config.timeout))??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ContextFilterError::Exit {
            status: output.status.to_string(),
            stderr: truncate_chars(stderr.trim(), STDERR_LOG_CHARS),
        });
    }
    let response: ContextFilterResponse = serde_json::from_slice(&output.stdout)?;
    Ok(response)
}

#[cfg(test)]
#[path = "context_filter_tests.rs"]
mod tests;
