//! HITL (Human-In-The-Loop) Outbox Holding Area & Quarantine Engine.
//!
//! Enforces architecture anti-corruption by intercepting external 3rd-party OSS defect reports,
//! serializing them into structured Minimal Reproducible Example (MRE) markdown drafts,
//! and enforcing strict human review/approval before any outbound transmission.

use crate::envelope::TaskEnvelope;
use crate::error::RouterError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Status of a staged issue draft in the HITL review lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StagedIssueStatus {
    /// Awaiting human inspection and sign-off.
    Pending,
    /// Explicitly approved by human engineer; authorized for outbound dispatch.
    Approved,
    /// Explicitly rejected by human engineer; barred from outbound transmission.
    Rejected,
}

/// Type alias for ergonomic status references.
pub type StagedStatus = StagedIssueStatus;

impl StagedIssueStatus {
    /// Returns `true` if the status is `Pending`.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Returns `true` if the status is `Approved`.
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }

    /// Returns `true` if the status is `Rejected`.
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected)
    }

    /// Returns standard metadata line value for this status.
    pub fn header_text(&self) -> &'static str {
        match self {
            Self::Pending => "Pending Human Authorization (HITL)",
            Self::Approved => "Approved by Human",
            Self::Rejected => "Rejected by Human",
        }
    }

    /// Returns standard markdown trailer text for this status.
    pub fn trailer_text(&self) -> &'static str {
        match self {
            Self::Pending => "*Status: Pending Human Authorization (HITL)*",
            Self::Approved => "*Status: Approved by Human*",
            Self::Rejected => "*Status: Rejected by Human*",
        }
    }
}

impl fmt::Display for StagedIssueStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Approved => write!(f, "Approved"),
            Self::Rejected => write!(f, "Rejected"),
        }
    }
}

/// Metadata summary of a staged issue draft in the HITL Outbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedIssueSummary {
    /// Task identifier (e.g. `task_9f82a1b4`).
    pub id: String,
    /// Bug report or task title.
    pub title: String,
    /// Target repository or component identifier.
    pub target: String,
    /// Severity level (e.g. `blocking`, `critical`, `high`, `medium`, `low`, `normal`).
    pub severity: String,
    /// Creation timestamp in RFC-3339 format.
    pub created_at: String,
    /// Current authorization status.
    pub status: StagedIssueStatus,
    /// File path to the staged markdown document on disk.
    pub file_path: PathBuf,
}

impl StagedIssueSummary {
    /// Convenience helper returning the path to the staged markdown document.
    pub fn path(&self) -> &Path {
        &self.file_path
    }
}

/// Sanitizes and resolves an issue identifier to a safe filename within `outbox_dir`.
///
/// Rejects path traversal tokens (`..`, `/`, `\`, `\0`) and empty strings
/// with `RouterError::IssueNotFound`. Transparently handles optional `issue_`
/// prefixes and `.md` file extensions.
fn resolve_issue_path(id: &str, outbox_dir: &Path) -> Result<(String, PathBuf), RouterError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(RouterError::IssueNotFound(id.to_string()));
    }

    // Defense-in-depth: Reject any path traversal tokens, slashes, or null bytes
    if trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(RouterError::IssueNotFound(id.to_string()));
    }

    let without_ext = trimmed.strip_suffix(".md").unwrap_or(trimmed);
    let raw_id = without_ext.strip_prefix("issue_").unwrap_or(without_ext);

    if raw_id.is_empty() || raw_id.starts_with('.') {
        return Err(RouterError::IssueNotFound(id.to_string()));
    }

    let candidate1 = outbox_dir.join(format!("issue_{raw_id}.md"));
    let candidate2 = outbox_dir.join(format!("issue_{without_ext}.md"));

    if candidate1.is_file() {
        Ok((raw_id.to_string(), candidate1))
    } else if candidate2.is_file() {
        Ok((without_ext.to_string(), candidate2))
    } else {
        Ok((raw_id.to_string(), candidate1))
    }
}

/// Helper to extract status from a status marker line.
///
/// Ensures mutually exclusive matching: lines containing ambiguous or conflicting
/// status keywords (e.g. both 'Approved' and 'Pending') return `None`.
fn parse_status_value(line: &str) -> Option<StagedIssueStatus> {
    let has_approved = line.contains("Approved");
    let has_rejected = line.contains("Rejected");
    let has_pending = line.contains("Pending");

    match (has_pending, has_approved, has_rejected) {
        (true, false, false) => Some(StagedIssueStatus::Pending),
        (false, true, false) => Some(StagedIssueStatus::Approved),
        (false, false, true) => Some(StagedIssueStatus::Rejected),
        _ => None,
    }
}

/// Detects the review status of an issue draft from its markdown contents.
///
/// Ensures status is detected strictly from the metadata header block (before any
/// `## ` section headers) or corroborated by the trailer after `---`.
fn detect_draft_status(content: &str) -> Option<StagedIssueStatus> {
    let mut header_status: Option<Option<StagedIssueStatus>> = None;
    let mut trailer_status: Option<Option<StagedIssueStatus>> = None;
    let mut in_header = true;
    let mut in_trailer = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_header = false;
        } else if trimmed.starts_with("---") {
            in_header = false;
            in_trailer = true;
        }

        if in_header {
            if let Some(rest) = trimmed.strip_prefix("- **Status**:") {
                let parsed = parse_status_value(rest);
                if header_status.is_none() {
                    header_status = Some(parsed);
                } else if header_status != Some(parsed) {
                    // Conflicting status lines within the header indicate tampering
                    return None;
                }
            }
        } else if in_trailer && trimmed.starts_with("*Status:") && trimmed.ends_with('*') {
            let parsed = parse_status_value(trimmed);
            if trailer_status.is_none() {
                trailer_status = Some(parsed);
            } else if trailer_status != Some(parsed) {
                return None;
            }
        }
    }

    match (header_status, trailer_status) {
        (Some(Some(h)), Some(Some(t))) => {
            if h == t {
                Some(h)
            } else {
                // Mismatch between header and trailer: tampering detected
                None
            }
        }
        (Some(Some(h)), None) => Some(h),
        (None, Some(Some(t))) => Some(t),
        // If either header or trailer was explicitly invalid (Some(None)), return None
        _ => None,
    }
}

/// Replaces existing status markers with the new status in markdown content.
fn update_draft_status_in_markdown(
    content: &str,
    new_status: StagedIssueStatus,
) -> Result<String, RouterError> {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut found_marker = false;
    let mut in_header = true;
    let mut in_trailer = false;

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_header = false;
        } else if trimmed.starts_with("---") {
            in_header = false;
            in_trailer = true;
        }

        if in_header && trimmed.starts_with("- **Status**:") {
            *line = format!("- **Status**: {}", new_status.header_text());
            found_marker = true;
        } else if in_trailer && trimmed.starts_with("*Status:") && trimmed.ends_with('*') {
            *line = new_status.trailer_text().to_string();
            found_marker = true;
        }
    }

    if !found_marker {
        return Err(RouterError::OutboxError(
            "Corrupted staged draft: missing status markers".to_string(),
        ));
    }

    let mut result = lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Parses summary metadata from a staged markdown draft.
fn parse_draft_summary(
    content: &str,
    fallback_id: &str,
    file_path: PathBuf,
) -> Option<StagedIssueSummary> {
    if content.trim().is_empty() {
        return None;
    }

    let mut title = None;
    let mut id = None;
    let mut target = None;
    let mut severity = None;
    let mut created_at = None;
    let status = detect_draft_status(content).unwrap_or(StagedIssueStatus::Pending);

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") || trimmed.starts_with("---") {
            break;
        }
        if let Some(t) = trimmed.strip_prefix("# Staged Issue Draft:") {
            if title.is_none() {
                title = Some(t.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("- **ID**:") {
            if id.is_none() {
                id = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("- **Target**:") {
            if target.is_none() {
                target = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("- **Severity**:") {
            if severity.is_none() {
                severity = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("- **Created At**:") {
            if created_at.is_none() {
                created_at = Some(rest.trim().to_string());
            }
        }
    }

    if title.is_none() && id.is_none() {
        return None;
    }

    Some(StagedIssueSummary {
        id: id.unwrap_or_else(|| fallback_id.to_string()),
        title: title.unwrap_or_else(|| "Untitled Staged Issue".to_string()),
        target: target.unwrap_or_else(|| "unknown".to_string()),
        severity: severity.unwrap_or_else(|| "normal".to_string()),
        created_at: created_at.unwrap_or_default(),
        status,
        file_path,
    })
}

/// HITL Outbox quarantine engine managing external defect reports.
pub struct HitlOutbox;

impl HitlOutbox {
    /// Stages an untrusted `TaskEnvelope` as a structured MRE markdown draft.
    ///
    /// Automatically creates `outbox_dir` and all parent directories if missing.
    /// Generates file at `{outbox_dir}/issue_{id}.md` with initial status `Pending`.
    /// Staging is idempotent: re-staging resets any previous status back to `Pending`.
    pub fn stage(envelope: &TaskEnvelope, outbox_dir: &Path) -> Result<PathBuf, RouterError> {
        envelope.validate()?;

        if envelope.id.contains('\0') || envelope.id.contains('\n') || envelope.id.contains('\r') {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'id' cannot contain null bytes or newlines".to_string(),
            ));
        }

        std::fs::create_dir_all(outbox_dir).map_err(|e| {
            RouterError::OutboxError(format!(
                "Failed to create outbox directory {:?}: {e}",
                outbox_dir
            ))
        })?;

        let file_path = outbox_dir.join(format!("issue_{}.md", envelope.id));

        let repro_json = match &envelope.payload.reproduction {
            Some(val) => serde_json::to_string_pretty(val)?,
            None => "null".to_string(),
        };

        let sanitized_title = envelope.payload.title.replace(['\r', '\n'], " ");
        let sanitized_target = envelope.target.identifier.replace(['\r', '\n'], " ");
        let sanitized_severity = envelope.payload.severity.replace(['\r', '\n'], " ");

        let content = format!(
            "# Staged Issue Draft: {}\n\n\
            - **ID**: {}\n\
            - **Target**: {}\n\
            - **Severity**: {}\n\
            - **Created At**: {}\n\
            - **Status**: Pending Human Authorization (HITL)\n\n\
            ## Reproduction\n\
            ```json\n\
            {}\n\
            ```\n\n\
            ## Expected\n\
            {}\n\n\
            ---\n\
            *Status: Pending Human Authorization (HITL)*\n",
            sanitized_title,
            envelope.id,
            sanitized_target,
            sanitized_severity,
            envelope.created_at,
            repro_json,
            envelope.payload.expected.as_deref().unwrap_or("N/A")
        );

        std::fs::write(&file_path, content)?;
        Ok(file_path)
    }

    /// Scans `outbox_dir` and extracts summary metadata for all staged drafts.
    ///
    /// Returns `Ok(vec![])` if `outbox_dir` does not exist or contains no drafts.
    /// Tolerates non-markdown files, subdirectories, empty/corrupt drafts, and OS junk.
    pub fn list_staged(outbox_dir: &Path) -> Result<Vec<StagedIssueSummary>, RouterError> {
        if !outbox_dir.exists() {
            return Ok(Vec::new());
        }

        let read_dir = match std::fs::read_dir(outbox_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(RouterError::Io(e)),
        };

        let mut summaries = Vec::new();

        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let fallback_id = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) if name.starts_with("issue_") && name.ends_with(".md") => {
                    name[6..name.len() - 3].to_string()
                }
                _ => continue,
            };

            if fallback_id.is_empty() {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if let Some(summary) = parse_draft_summary(&content, &fallback_id, path) {
                summaries.push(summary);
            }
        }

        summaries.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(summaries)
    }

    /// Reads and returns the complete markdown content of a staged draft for a given issue ID.
    ///
    /// Returns `RouterError::IssueNotFound` if the issue draft does not exist.
    pub fn inspect_staged(id: &str, outbox_dir: &Path) -> Result<String, RouterError> {
        let (_raw_id, path) = resolve_issue_path(id, outbox_dir)?;
        if !path.is_file() {
            return Err(RouterError::IssueNotFound(id.to_string()));
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(content)
    }

    /// Authorizes a staged draft, transitioning status from `Pending` to `Approved`.
    ///
    /// Returns `RouterError::IssueNotFound` if the draft is missing.
    /// Returns `RouterError::OutboxError` if the draft is already Approved or Rejected.
    pub fn approve(id: &str, outbox_dir: &Path) -> Result<PathBuf, RouterError> {
        let (raw_id, path) = resolve_issue_path(id, outbox_dir)?;
        if !path.is_file() {
            return Err(RouterError::IssueNotFound(id.to_string()));
        }

        let content = std::fs::read_to_string(&path)?;
        let current_status = detect_draft_status(&content).ok_or_else(|| {
            RouterError::OutboxError(format!(
                "Corrupted or missing status in issue draft '{raw_id}'"
            ))
        })?;

        match current_status {
            StagedIssueStatus::Approved => {
                return Err(RouterError::OutboxError(format!(
                    "Issue '{raw_id}' is already approved"
                )));
            }
            StagedIssueStatus::Rejected => {
                return Err(RouterError::OutboxError(format!(
                    "Cannot approve issue '{raw_id}': status is Rejected"
                )));
            }
            StagedIssueStatus::Pending => {}
        }

        let updated_content =
            update_draft_status_in_markdown(&content, StagedIssueStatus::Approved)?;
        std::fs::write(&path, updated_content)?;
        Ok(path)
    }

    /// Explicitly rejects a staged draft, transitioning status from `Pending` to `Rejected`.
    ///
    /// Preserves the draft on disk as an audit trail.
    /// Returns `RouterError::IssueNotFound` if the draft is missing.
    /// Returns `RouterError::OutboxError` if the draft is already Rejected or Approved.
    pub fn reject(id: &str, outbox_dir: &Path) -> Result<(), RouterError> {
        let (raw_id, path) = resolve_issue_path(id, outbox_dir)?;
        if !path.is_file() {
            return Err(RouterError::IssueNotFound(id.to_string()));
        }

        let content = std::fs::read_to_string(&path)?;
        let current_status = detect_draft_status(&content).ok_or_else(|| {
            RouterError::OutboxError(format!(
                "Corrupted or missing status in issue draft '{raw_id}'"
            ))
        })?;

        match current_status {
            StagedIssueStatus::Rejected => {
                return Err(RouterError::OutboxError(format!(
                    "Issue '{raw_id}' is already rejected"
                )));
            }
            StagedIssueStatus::Approved => {
                return Err(RouterError::OutboxError(format!(
                    "Cannot reject issue '{raw_id}': status is already Approved"
                )));
            }
            StagedIssueStatus::Pending => {}
        }

        let updated_content =
            update_draft_status_in_markdown(&content, StagedIssueStatus::Rejected)?;
        std::fs::write(&path, updated_content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{CallerInfo, TargetInfo, TaskPayload};
    use tempfile::TempDir;

    fn sample_envelope(id: &str, title: &str) -> TaskEnvelope {
        TaskEnvelope {
            schema: None,
            id: id.to_string(),
            created_at: "2026-10-11T10:00:00Z".to_string(),
            caller: CallerInfo {
                workspace: "/test/ws".to_string(),
                agent: "test_caller".to_string(),
                pane_id: None,
            },
            target: TargetInfo {
                identifier: "external-upstream".to_string(),
                workspace: None,
                is_external: true,
            },
            intent: "bug_report".to_string(),
            payload: TaskPayload {
                title: title.to_string(),
                severity: "high".to_string(),
                reproduction: Some(serde_json::json!({
                    "snippet": "const btn = render();",
                    "error": "Button render panic"
                })),
                expected: Some("Clean button rendering".to_string()),
            },
        }
    }

    #[test]
    fn test_stage_canonical_envelope() {
        let temp_dir = TempDir::new().unwrap();
        let outbox_dir = temp_dir.path().join("staged");

        let env = sample_envelope("task_alpha", "Button Crash");
        let path = HitlOutbox::stage(&env, &outbox_dir).expect("Stage must succeed");

        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "issue_task_alpha.md");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("# Staged Issue Draft: Button Crash"));
        assert!(content.contains("- **ID**: task_alpha"));
        assert!(content.contains("- **Target**: external-upstream"));
        assert!(content.contains("- **Severity**: high"));
        assert!(content.contains("- **Created At**: 2026-10-11T10:00:00Z"));
        assert!(content.contains("- **Status**: Pending Human Authorization (HITL)"));
        assert!(content.contains("const btn = render();"));
        assert!(content.contains("Clean button rendering"));
        assert!(content.contains("*Status: Pending Human Authorization (HITL)*"));
    }

    #[test]
    fn test_stage_auto_creates_directories() {
        let temp_dir = TempDir::new().unwrap();
        let nested = temp_dir.path().join("a").join("b").join("c");

        let env = sample_envelope("task_nested", "Nested Draft");
        let path = HitlOutbox::stage(&env, &nested).expect("Must auto-create directories");
        assert!(path.exists());
    }

    #[test]
    fn test_stage_idempotent_overwrite_resets_status() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_reset", "Initial Title");
        let path = HitlOutbox::stage(&env, &outbox).unwrap();

        // Approve it first
        HitlOutbox::approve("task_reset", &outbox).unwrap();
        let approved_content = std::fs::read_to_string(&path).unwrap();
        assert!(approved_content.contains("Approved by Human"));

        // Re-stage should reset to Pending
        let mut env2 = env.clone();
        env2.payload.title = "Updated Title".to_string();
        HitlOutbox::stage(&env2, &outbox).unwrap();

        let reset_content = std::fs::read_to_string(&path).unwrap();
        assert!(reset_content.contains("Updated Title"));
        assert!(reset_content.contains("Pending Human Authorization (HITL)"));
        assert!(!reset_content.contains("Approved by Human"));
    }

    #[test]
    fn test_stage_none_optional_fields() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let mut env = sample_envelope("task_minimal", "Minimal Issue");
        env.payload.reproduction = None;
        env.payload.expected = None;

        let path = HitlOutbox::stage(&env, &outbox).unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("null"));
        assert!(content.contains("## Expected\nN/A"));
    }

    #[test]
    fn test_list_staged_nonexistent_directory_returns_empty() {
        let temp_dir = TempDir::new().unwrap();
        let non_existent = temp_dir.path().join("does_not_exist");

        let summaries = HitlOutbox::list_staged(&non_existent).unwrap();
        assert!(summaries.is_empty());
    }

    #[test]
    fn test_list_staged_multiple_drafts_sorted() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let mut env1 = sample_envelope("task_1", "Issue 1");
        env1.created_at = "2026-10-11T10:00:00Z".to_string();

        let mut env2 = sample_envelope("task_2", "Issue 2");
        env2.created_at = "2026-10-11T12:00:00Z".to_string();

        let mut env3 = sample_envelope("task_3", "Issue 3");
        env3.created_at = "2026-10-11T11:00:00Z".to_string();

        HitlOutbox::stage(&env1, &outbox).unwrap();
        HitlOutbox::stage(&env2, &outbox).unwrap();
        HitlOutbox::stage(&env3, &outbox).unwrap();

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries.len(), 3);
        // Sorted descending by created_at: task_2 (12:00), task_3 (11:00), task_1 (10:00)
        assert_eq!(summaries[0].id, "task_2");
        assert_eq!(summaries[1].id, "task_3");
        assert_eq!(summaries[2].id, "task_1");
        assert_eq!(summaries[0].status, StagedIssueStatus::Pending);
    }

    #[test]
    fn test_list_staged_ignores_junk_files_and_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");
        std::fs::create_dir_all(&outbox).unwrap();

        // Valid stage
        let env = sample_envelope("task_valid", "Valid");
        HitlOutbox::stage(&env, &outbox).unwrap();

        // Non-draft files & subdirs
        std::fs::write(outbox.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(outbox.join("README.md"), b"# Notes").unwrap();
        std::fs::write(outbox.join("random.txt"), b"some text").unwrap();
        std::fs::create_dir(outbox.join("nested_dir")).unwrap();

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "task_valid");
    }

    #[test]
    fn test_list_staged_skips_empty_or_corrupt_files() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");
        std::fs::create_dir_all(&outbox).unwrap();

        let env = sample_envelope("task_real", "Real Issue");
        HitlOutbox::stage(&env, &outbox).unwrap();

        // Corrupted / 0-byte file
        std::fs::write(outbox.join("issue_empty.md"), b"").unwrap();
        std::fs::write(outbox.join("issue_corrupt.md"), b"not a draft").unwrap();

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "task_real");
    }

    #[test]
    fn test_inspect_staged_success_and_normalization() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_42", "Issue 42");
        HitlOutbox::stage(&env, &outbox).unwrap();

        // Query via raw ID
        let content1 = HitlOutbox::inspect_staged("task_42", &outbox).unwrap();
        assert!(content1.contains("Issue 42"));

        // Query via prefixed ID
        let content2 = HitlOutbox::inspect_staged("issue_task_42", &outbox).unwrap();
        assert_eq!(content1, content2);

        // Query via filename with extension
        let content3 = HitlOutbox::inspect_staged("issue_task_42.md", &outbox).unwrap();
        assert_eq!(content1, content3);
    }

    #[test]
    fn test_inspect_staged_missing_id() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let err = HitlOutbox::inspect_staged("non_existent", &outbox).unwrap_err();
        match err {
            RouterError::IssueNotFound(id) => assert_eq!(id, "non_existent"),
            other => panic!("Expected IssueNotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_inspect_staged_path_traversal_blocked() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        assert!(matches!(
            HitlOutbox::inspect_staged("../../etc/passwd", &outbox).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
        assert!(matches!(
            HitlOutbox::inspect_staged("/etc/passwd", &outbox).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
        assert!(matches!(
            HitlOutbox::inspect_staged("task\0evil", &outbox).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
        assert!(matches!(
            HitlOutbox::inspect_staged("   ", &outbox).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
    }

    #[test]
    fn test_approve_lifecycle_transition() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_approve", "Approve Me");
        let path = HitlOutbox::stage(&env, &outbox).unwrap();

        let approved_path = HitlOutbox::approve("task_approve", &outbox).unwrap();
        assert_eq!(path, approved_path);

        let content = std::fs::read_to_string(&approved_path).unwrap();
        assert!(content.contains("- **Status**: Approved by Human"));
        assert!(content.contains("*Status: Approved by Human*"));
        assert!(!content.contains("Pending Human Authorization (HITL)"));

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries[0].status, StagedIssueStatus::Approved);
    }

    #[test]
    fn test_approve_already_approved_fails() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_double_approve", "Double Approve");
        HitlOutbox::stage(&env, &outbox).unwrap();

        HitlOutbox::approve("task_double_approve", &outbox).unwrap();
        let err = HitlOutbox::approve("task_double_approve", &outbox).unwrap_err();

        match err {
            RouterError::OutboxError(msg) => {
                assert!(msg.contains("already approved"));
            }
            other => panic!("Expected OutboxError, got {other:?}"),
        }
    }

    #[test]
    fn test_approve_rejected_fails() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_rej_app", "Reject then Approve");
        HitlOutbox::stage(&env, &outbox).unwrap();

        HitlOutbox::reject("task_rej_app", &outbox).unwrap();
        let err = HitlOutbox::approve("task_rej_app", &outbox).unwrap_err();

        match err {
            RouterError::OutboxError(msg) => {
                assert!(msg.contains("status is Rejected"));
            }
            other => panic!("Expected OutboxError, got {other:?}"),
        }
    }

    #[test]
    fn test_reject_lifecycle_transition() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_reject", "Reject Me");
        let path = HitlOutbox::stage(&env, &outbox).unwrap();

        HitlOutbox::reject("task_reject", &outbox).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("- **Status**: Rejected by Human"));
        assert!(content.contains("*Status: Rejected by Human*"));
        assert!(!content.contains("Pending Human Authorization (HITL)"));

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries[0].status, StagedIssueStatus::Rejected);
    }

    #[test]
    fn test_reject_already_rejected_fails() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_double_reject", "Double Reject");
        HitlOutbox::stage(&env, &outbox).unwrap();

        HitlOutbox::reject("task_double_reject", &outbox).unwrap();
        let err = HitlOutbox::reject("task_double_reject", &outbox).unwrap_err();

        match err {
            RouterError::OutboxError(msg) => {
                assert!(msg.contains("already rejected"));
            }
            other => panic!("Expected OutboxError, got {other:?}"),
        }
    }

    #[test]
    fn test_reject_approved_fails() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_app_rej", "Approve then Reject");
        HitlOutbox::stage(&env, &outbox).unwrap();

        HitlOutbox::approve("task_app_rej", &outbox).unwrap();
        let err = HitlOutbox::reject("task_app_rej", &outbox).unwrap_err();

        match err {
            RouterError::OutboxError(msg) => {
                assert!(msg.contains("already Approved"));
            }
            other => panic!("Expected OutboxError, got {other:?}"),
        }
    }

    #[test]
    fn test_staged_issue_status_helpers_and_display() {
        let pending = StagedIssueStatus::Pending;
        assert!(pending.is_pending());
        assert!(!pending.is_approved());
        assert!(!pending.is_rejected());
        assert_eq!(pending.to_string(), "Pending");

        let approved = StagedIssueStatus::Approved;
        assert!(approved.is_approved());
        assert_eq!(approved.to_string(), "Approved");

        let rejected = StagedIssueStatus::Rejected;
        assert!(rejected.is_rejected());
        assert_eq!(rejected.to_string(), "Rejected");

        let json = serde_json::to_string(&approved).unwrap();
        let des: StagedIssueStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(des, StagedIssueStatus::Approved);
    }

    #[test]
    fn test_staged_issue_summary_path_helper_and_serde() {
        let summary = StagedIssueSummary {
            id: "task_100".to_string(),
            title: "Summary Test".to_string(),
            target: "pkg".to_string(),
            severity: "normal".to_string(),
            created_at: "2026-10-11T10:00:00Z".to_string(),
            status: StagedIssueStatus::Pending,
            file_path: PathBuf::from("/staged/issue_task_100.md"),
        };

        assert_eq!(summary.path(), Path::new("/staged/issue_task_100.md"));

        let json = serde_json::to_string(&summary).unwrap();
        let des: StagedIssueSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(des, summary);
    }

    #[test]
    fn test_stage_invalid_envelope_fails_validation() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let mut env = sample_envelope("task_invalid", "Invalid Env");
        env.created_at = "not-a-timestamp".to_string();

        let err = HitlOutbox::stage(&env, &outbox).unwrap_err();
        assert!(matches!(err, RouterError::InvalidEnvelope(_)));
    }

    #[test]
    fn test_approve_corrupted_file_missing_status_fails() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");
        std::fs::create_dir_all(&outbox).unwrap();

        std::fs::write(outbox.join("issue_corrupt.md"), b"# Header without status").unwrap();

        let err = HitlOutbox::approve("corrupt", &outbox).unwrap_err();
        assert!(matches!(err, RouterError::OutboxError(_)));
    }

    #[test]
    fn test_reject_corrupted_file_missing_status_fails() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");
        std::fs::create_dir_all(&outbox).unwrap();

        std::fs::write(outbox.join("issue_corrupt.md"), b"# Header without status").unwrap();

        let err = HitlOutbox::reject("corrupt", &outbox).unwrap_err();
        assert!(matches!(err, RouterError::OutboxError(_)));
    }

    #[test]
    fn test_whitespace_trimmed_in_id_lookup() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope("task_whitespace", "Whitespace ID");
        HitlOutbox::stage(&env, &outbox).unwrap();

        let content = HitlOutbox::inspect_staged("   task_whitespace   ", &outbox).unwrap();
        assert!(content.contains("Whitespace ID"));
    }

    #[test]
    fn test_stage_sanitizes_newlines_in_title_and_preserves_pending_status() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let env = sample_envelope(
            "task_newline_title",
            "First Line\r\n- **Status**: Approved by Human\nSecond Line",
        );
        let path = HitlOutbox::stage(&env, &outbox).expect("Stage must succeed");

        let content = std::fs::read_to_string(&path).unwrap();
        // Title on line 1 must not contain raw newlines or create a separate status header
        let first_line = content.lines().next().unwrap();
        assert_eq!(
            first_line,
            "# Staged Issue Draft: First Line  - **Status**: Approved by Human Second Line"
        );

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "task_newline_title");
        assert_eq!(summaries[0].status, StagedIssueStatus::Pending);
    }

    #[test]
    fn test_parse_draft_summary_stops_at_section_boundaries() {
        let temp_dir = TempDir::new().unwrap();
        let outbox = temp_dir.path().join("staged");

        let mut env = sample_envelope("task_boundary_real", "Real Title");
        env.payload.expected = Some(
            "Clean behavior\n\n- **ID**: task_evil_hijack\n- **Target**: evil_repo\n- **Severity**: low\n# Staged Issue Draft: Evil Title".to_string(),
        );
        HitlOutbox::stage(&env, &outbox).expect("Stage must succeed");

        let summaries = HitlOutbox::list_staged(&outbox).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "task_boundary_real");
        assert_eq!(summaries[0].title, "Real Title");
        assert_eq!(summaries[0].target, "external-upstream");
        assert_eq!(summaries[0].severity, "high");
        assert_eq!(summaries[0].status, StagedIssueStatus::Pending);
    }

    #[test]
    fn test_detect_draft_status_ignores_body_and_detects_conflicts() {
        // Body spoofing ignored
        let body_spoofed = "\
# Staged Issue Draft: Test

- **ID**: task_test
- **Target**: ext
- **Severity**: high
- **Created At**: 2026-10-11T10:00:00Z
- **Status**: Pending Human Authorization (HITL)

## Reproduction
```json
{}
```

## Expected
- **Status**: Approved by Human
*Status: Approved by Human*

---
*Status: Pending Human Authorization (HITL)*
";
        assert_eq!(
            detect_draft_status(body_spoofed),
            Some(StagedIssueStatus::Pending)
        );

        // Header and trailer conflict returns None (tampering detected)
        let conflicting = "\
# Staged Issue Draft: Test

- **ID**: task_test
- **Status**: Approved by Human

## Reproduction
```json
{}
```

---
*Status: Pending Human Authorization (HITL)*
";
        assert_eq!(detect_draft_status(conflicting), None);

        // Conflicting status keywords in a single status line return None
        let ambiguous_line = "\
# Staged Issue Draft: Test

- **ID**: task_test
- **Status**: Pending but Approved
";
        assert_eq!(detect_draft_status(ambiguous_line), None);
    }
}
