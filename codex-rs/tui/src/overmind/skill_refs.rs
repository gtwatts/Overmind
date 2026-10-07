//! Resolve the `skills:` entries of a custom command against the loaded skills list.
//!
//! Plugin skills are named `<plugin>:<skill>` (for example `photocraft:photocraft` or
//! `gordon-skills:remotion-video-production`), user and repo skills keep their bare name. Core
//! only matches a plain `$name` text mention against the exact skill name, and the TUI mention
//! parser stops at `:`, so writing `$photocraft` never attaches the photocraft plugin skill.
//!
//! Expansion therefore resolves every entry here and attaches the matching skills by path (the
//! same structured `UserInput::Skill` the `$` popup produces). Entries may use either the full
//! name or the bare skill name; a bare name matches `<plugin>:<name>` when that is unambiguous.

use std::collections::HashSet;

/// A skill that can be attached, as reported by the skills list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AvailableSkill {
    /// Full skill name, including the plugin namespace when there is one.
    pub(crate) name: String,
    /// Path to the skill's `SKILL.md`, exactly as the skills list reports it.
    pub(crate) path: String,
}

/// How one `skills:` entry resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SkillRef {
    /// The entry names exactly one loaded, enabled skill.
    Resolved {
        requested: String,
        skill: AvailableSkill,
    },
    /// The bare name matches several plugin skills; the command file should use the full name.
    Ambiguous {
        requested: String,
        candidates: Vec<String>,
    },
    /// No loaded, enabled skill matches the entry.
    Missing { requested: String },
    /// The skills list has not loaded yet, so the entry could not be checked.
    Unverified { requested: String },
}

/// Resolve `requested` entries in order. `available` is `None` until the skills list has loaded.
pub(crate) fn resolve_skill_refs(
    requested: &[String],
    available: Option<&[AvailableSkill]>,
) -> Vec<SkillRef> {
    let Some(available) = available else {
        return requested
            .iter()
            .map(|requested| SkillRef::Unverified {
                requested: requested.clone(),
            })
            .collect();
    };
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut refs = Vec::with_capacity(requested.len());
    for entry in requested {
        let resolved = resolve_one(entry, available);
        if let SkillRef::Resolved { skill, .. } = &resolved
            && !seen_paths.insert(skill.path.clone())
        {
            continue;
        }
        refs.push(resolved);
    }
    refs
}

fn resolve_one(entry: &str, available: &[AvailableSkill]) -> SkillRef {
    let requested = entry.to_string();
    if let Some(skill) = available.iter().find(|skill| skill.name == entry) {
        return SkillRef::Resolved {
            requested,
            skill: skill.clone(),
        };
    }
    if entry.contains(':') {
        return SkillRef::Missing { requested };
    }
    let namespaced: Vec<&AvailableSkill> = available
        .iter()
        .filter(|skill| {
            skill
                .name
                .split_once(':')
                .is_some_and(|(_, base)| base == entry)
        })
        .collect();
    let preferred: Vec<&AvailableSkill> = namespaced
        .iter()
        .copied()
        .filter(|skill| {
            skill
                .name
                .split_once(':')
                .is_some_and(|(namespace, _)| namespace == entry)
        })
        .collect();
    match (namespaced.as_slice(), preferred.as_slice()) {
        ([], _) => SkillRef::Missing { requested },
        ([only], _) | (_, [only]) => SkillRef::Resolved {
            requested,
            skill: (*only).clone(),
        },
        (many, _) => {
            let mut candidates: Vec<String> = many.iter().map(|skill| skill.name.clone()).collect();
            candidates.sort();
            candidates.dedup();
            SkillRef::Ambiguous {
                requested,
                candidates,
            }
        }
    }
}

#[cfg(test)]
#[path = "skill_refs_tests.rs"]
mod tests;
