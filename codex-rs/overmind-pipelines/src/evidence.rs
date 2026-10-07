//! Output evidence: what a finished stage produced, and whether it is still there unchanged.
//!
//! Files are bound by SHA-256; size and modification time act as a fast path so checking a
//! run does not rehash large renders that did not change. Directories (`production/boards/`)
//! are bound by a digest of their recursive listing (paths, sizes, modification times) and
//! must not be empty, matching the ledger rule that evidence is non-empty.

use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;

use crate::hashing::sha256_file;
use crate::hashing::sha256_hex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    File,
    Dir,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// Path relative to the run's workspace, as declared by the stage.
    pub path: String,
    pub kind: EvidenceKind,
    pub sha256: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ns: Option<u64>,
}

/// Evidence for `output` under `workspace`, or `None` when it is missing or empty.
pub(crate) fn collect(workspace: &Path, output: &str) -> Option<Evidence> {
    let path = workspace.join(output.trim_end_matches('/'));
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() {
        return None;
    }
    if metadata.is_dir() {
        let mut entries = Vec::new();
        list_dir(&path, &path, &mut entries);
        if entries.is_empty() {
            return None;
        }
        entries.sort();
        let bytes = entries.iter().map(|(_, size, _)| *size).sum();
        let listing: String = entries
            .iter()
            .map(|(rel, size, modified)| format!("{rel}\0{size}\0{modified}\n"))
            .collect();
        return Some(Evidence {
            path: output.to_string(),
            kind: EvidenceKind::Dir,
            sha256: sha256_hex(listing.as_bytes()),
            bytes,
            modified_ns: None,
        });
    }
    if !metadata.is_file() || metadata.len() == 0 {
        return None;
    }
    Some(Evidence {
        path: output.to_string(),
        kind: EvidenceKind::File,
        sha256: sha256_file(&path).ok()?,
        bytes: metadata.len(),
        modified_ns: modified_ns(&metadata),
    })
}

/// Whether recorded evidence still matches the workspace.
pub(crate) fn still_valid(workspace: &Path, evidence: &Evidence) -> bool {
    match evidence.kind {
        EvidenceKind::Dir => collect(workspace, &evidence.path)
            .is_some_and(|current| current.sha256 == evidence.sha256),
        EvidenceKind::File => {
            let path = workspace.join(&evidence.path);
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                return false;
            };
            if !metadata.is_file() || metadata.len() != evidence.bytes {
                return false;
            }
            if evidence.modified_ns.is_some() && modified_ns(&metadata) == evidence.modified_ns {
                return true;
            }
            sha256_file(&path).is_ok_and(|sha| sha == evidence.sha256)
        }
    }
}

fn modified_ns(metadata: &std::fs::Metadata) -> Option<u64> {
    let since_epoch = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since_epoch.as_nanos()).ok()
}

fn list_dir(root: &Path, dir: &Path, out: &mut Vec<(String, u64, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            list_dir(root, &path, out);
        } else if metadata.is_file() {
            let rel = path
                .strip_prefix(root)
                .map(|rel| rel.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push((rel, metadata.len(), modified_ns(&metadata).unwrap_or(0)));
        }
    }
}
