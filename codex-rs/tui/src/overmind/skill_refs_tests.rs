use pretty_assertions::assert_eq;

use super::*;

fn skill(name: &str, path: &str) -> AvailableSkill {
    AvailableSkill {
        name: name.to_string(),
        path: path.to_string(),
    }
}

fn requested(entries: &[&str]) -> Vec<String> {
    entries.iter().map(|entry| (*entry).to_string()).collect()
}

/// Mirrors how the skills list names Todd's video skills: user skills are bare, plugin skills
/// are `<plugin>:<skill>`.
fn loaded_skills() -> Vec<AvailableSkill> {
    vec![
        skill("video", "/home/u/.codex/skills/video/SKILL.md"),
        skill(
            "photocraft:photocraft",
            "/home/u/.codex/plugins/cache/personal/photocraft/0.3.0/skills/photocraft/SKILL.md",
        ),
        skill(
            "blender-mcp:blender",
            "/home/u/.codex/plugins/cache/personal/blender-mcp/0.1.0/skills/blender/SKILL.md",
        ),
        skill(
            "gordon-skills:remotion-video-production",
            "/home/u/.codex/plugins/cache/gordon-codex/gordon-skills/1.0.1/skills/remotion-video-production/SKILL.md",
        ),
        skill(
            "gordon-workflows:high-end-whiteboard",
            "/home/u/.codex/plugins/cache/gordon-codex/gordon-workflows/1.0.0/skills/high-end-whiteboard/SKILL.md",
        ),
        skill("a:shared", "/a/shared/SKILL.md"),
        skill("b:shared", "/b/shared/SKILL.md"),
    ]
}

#[test]
fn resolves_user_skills_plugin_skills_and_full_names() {
    let available = loaded_skills();
    let refs = resolve_skill_refs(
        &requested(&[
            "video",
            "photocraft",
            "blender",
            "remotion-video-production",
            "gordon-workflows:high-end-whiteboard",
        ]),
        Some(&available),
    );

    assert_eq!(
        refs,
        vec![
            SkillRef::Resolved {
                requested: "video".to_string(),
                skill: available[0].clone(),
            },
            SkillRef::Resolved {
                requested: "photocraft".to_string(),
                skill: available[1].clone(),
            },
            SkillRef::Resolved {
                requested: "blender".to_string(),
                skill: available[2].clone(),
            },
            SkillRef::Resolved {
                requested: "remotion-video-production".to_string(),
                skill: available[3].clone(),
            },
            SkillRef::Resolved {
                requested: "gordon-workflows:high-end-whiteboard".to_string(),
                skill: available[4].clone(),
            },
        ]
    );
}

#[test]
fn reports_missing_ambiguous_and_duplicate_entries() {
    let available = loaded_skills();
    let refs = resolve_skill_refs(
        &requested(&[
            "shared",
            "nope",
            "nope:photocraft",
            "photocraft",
            "photocraft:photocraft",
        ]),
        Some(&available),
    );

    assert_eq!(
        refs,
        vec![
            SkillRef::Ambiguous {
                requested: "shared".to_string(),
                candidates: vec!["a:shared".to_string(), "b:shared".to_string()],
            },
            SkillRef::Missing {
                requested: "nope".to_string(),
            },
            SkillRef::Missing {
                requested: "nope:photocraft".to_string(),
            },
            SkillRef::Resolved {
                requested: "photocraft".to_string(),
                skill: available[1].clone(),
            },
        ]
    );
}

#[test]
fn bare_name_prefers_the_plugin_named_after_the_skill() {
    let available = vec![
        skill("photocraft:photocraft", "/p/photocraft/SKILL.md"),
        skill("other:photocraft", "/o/photocraft/SKILL.md"),
    ];
    assert_eq!(
        resolve_skill_refs(&requested(&["photocraft"]), Some(&available)),
        vec![SkillRef::Resolved {
            requested: "photocraft".to_string(),
            skill: available[0].clone(),
        }]
    );
}

#[test]
fn unloaded_skills_list_leaves_entries_unverified() {
    assert_eq!(
        resolve_skill_refs(&requested(&["photocraft"]), None),
        vec![SkillRef::Unverified {
            requested: "photocraft".to_string(),
        }]
    );
}
