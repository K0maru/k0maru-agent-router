//! Execution Receipt Protocol (`agent-receipt-v1`) and Verification Engine.
//!
//! Conforms to SPEC.md §6 and https://k0maru.dev/schemas/agent-receipt-v1.json.
//! Ingests, cryptographically validates, and persists downstream agent execution receipts.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::error::RouterError;

/// Authoritative JSON Schema URI for execution receipts.
pub const DEFAULT_RECEIPT_SCHEMA: &str = "https://k0maru.dev/schemas/agent-receipt-v1.json";

/// Environment variable controlling global receipt bus storage location.
pub const ENV_K0MARU_RECEIPT_DIR: &str = "K0MARU_RECEIPT_DIR";

/// Default hidden root folder name under user's home directory.
pub const DEFAULT_K0MARU_DIR: &str = ".k0maru";

/// Default subfolder name for execution receipts storage.
pub const DEFAULT_RECEIPTS_DIR: &str = "receipts";

/// Resolves the canonical receipt storage directory based on standard precedence:
/// 1. Priority 1: Explicit override path or CLI `--receipt-dir <PATH>` if supplied
/// 2. Priority 2: Environment variable `K0MARU_RECEIPT_DIR`
/// 3. Priority 3: Default home directory `dirs::home_dir().join(".k0maru").join("receipts")`
///    (fallback to current dir `.k0maru/receipts` if home dir cannot be resolved)
pub fn resolve_receipts_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }

    if let Ok(env_val) = std::env::var(ENV_K0MARU_RECEIPT_DIR) {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if let Some(home) = dirs::home_dir() {
        home.join(DEFAULT_K0MARU_DIR).join(DEFAULT_RECEIPTS_DIR)
    } else {
        PathBuf::from(DEFAULT_K0MARU_DIR).join(DEFAULT_RECEIPTS_DIR)
    }
}

/// Verdict of downstream agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptVerdict {
    /// Agent execution succeeded and all checks passed.
    Success,
    /// Execution failed (e.g. compiler error, test failures, lint errors).
    Failed,
    /// Execution was blocked by external constraint or missing dependency.
    Blocked,
}

impl ReceiptVerdict {
    /// Returns `true` if the verdict is `Success`.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Returns `true` if the verdict is `Failed`.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Returns `true` if the verdict is `Blocked`.
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked)
    }

    /// Returns the canonical lowercase representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

impl fmt::Display for ReceiptVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for ReceiptVerdict {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReceiptVerdict {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.trim().to_ascii_lowercase().as_str() {
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            other => Err(serde::de::Error::custom(format!(
                "Invalid receipt verdict: '{other}'. Expected 'success', 'failed', or 'blocked'"
            ))),
        }
    }
}

/// Compilation check results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilationCheck {
    /// Check status (e.g. "passed" or "failed").
    pub status: String,
    /// Exit code of the compiler process (0 indicates success).
    pub exit_code: i32,
}

impl CompilationCheck {
    /// Returns `true` if compilation succeeded.
    pub fn is_passed(&self) -> bool {
        self.status.eq_ignore_ascii_case("passed") && self.exit_code == 0
    }
}

/// Test runner execution results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCheck {
    /// Check status (e.g. "passed" or "failed").
    pub status: String,
    /// Total test cases executed.
    pub total: usize,
    /// Number of failing test cases.
    pub failed: usize,
    /// Execution duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl TestCheck {
    /// Returns `true` if all tests passed with zero failures.
    pub fn is_passed(&self) -> bool {
        self.status.eq_ignore_ascii_case("passed") && self.failed == 0
    }
}

/// Linter and static analysis results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinterCheck {
    /// Check status (e.g. "passed" or "failed").
    pub status: String,
    /// Number of reported linter warnings.
    pub warnings: usize,
}

impl LinterCheck {
    /// Returns `true` if linter passed with zero warnings.
    pub fn is_passed(&self) -> bool {
        self.status.eq_ignore_ascii_case("passed") && self.warnings == 0
    }
}

/// Scope guard check ensuring changes remained within allowed boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeGuardCheck {
    /// Check status (e.g. "passed" or "failed").
    pub status: String,
    /// List of modified file paths.
    #[serde(default)]
    pub modified_files: Vec<String>,
}

impl ScopeGuardCheck {
    /// Returns `true` if scope guard checks passed.
    pub fn is_passed(&self) -> bool {
        self.status.eq_ignore_ascii_case("passed")
    }
}

/// Aggregated execution checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionChecks {
    /// Compilation check.
    pub compilation: CompilationCheck,
    /// Automated test execution check.
    pub tests: TestCheck,
    /// Linter / static analysis check.
    pub linter: LinterCheck,
    /// Boundary / scope guard check.
    pub scope_guard: ScopeGuardCheck,
}

impl ExecutionChecks {
    /// Returns `true` if all four checks passed.
    pub fn is_all_passed(&self) -> bool {
        self.compilation.is_passed()
            && self.tests.is_passed()
            && self.linter.is_passed()
            && self.scope_guard.is_passed()
    }
}

/// Artifacts associated with receipt execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReceiptArtifacts {
    /// Optional SHA-256 digest of git diff / patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_patch_sha256: Option<String>,
    /// Receipt generation timestamp in RFC-3339 format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_timestamp: Option<String>,
}

fn default_receipt_schema() -> String {
    DEFAULT_RECEIPT_SCHEMA.to_string()
}

/// Full execution receipt conforming to `https://k0maru.dev/schemas/agent-receipt-v1.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    /// Schema URI reference.
    #[serde(rename = "$schema", default = "default_receipt_schema")]
    pub schema: String,
    /// Task identifier corresponding to the executed `TaskEnvelope`.
    pub task_id: String,
    /// Working directory or workspace sandbox where the task was executed.
    pub workspace: PathBuf,
    /// Execution verdict.
    pub verdict: ReceiptVerdict,
    /// Individual check verification results.
    pub checks: ExecutionChecks,
    /// Execution artifacts and cryptographic digests.
    #[serde(default)]
    pub artifacts: ReceiptArtifacts,
}

/// Human-readable and machine-digestible summary of an execution receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptSummary {
    /// Task identifier.
    pub task_id: String,
    /// Verdict.
    pub verdict: ReceiptVerdict,
    /// Whether all checks and the verdict are passing.
    pub is_passing: bool,
    /// Workspace path.
    pub workspace: PathBuf,
    /// Condensed summary of check results.
    pub checks_summary: String,
    /// Receipt generation timestamp if present.
    pub receipt_timestamp: Option<String>,
    /// Cryptographic patch SHA-256 if present.
    pub git_patch_sha256: Option<String>,
}

impl ExecutionReceipt {
    /// Validates the internal consistency and schema invariants of the receipt.
    pub fn validate(&self) -> Result<(), RouterError> {
        let task_id = self.task_id.trim();
        if task_id.is_empty() {
            return Err(RouterError::InvalidReceipt(
                "Receipt 'task_id' cannot be empty".to_string(),
            ));
        }

        // Reject path traversal tokens, slashes, or null bytes
        if task_id.contains("..")
            || task_id.contains('/')
            || task_id.contains('\\')
            || task_id.contains('\0')
        {
            return Err(RouterError::InvalidReceipt(format!(
                "Receipt 'task_id' contains invalid path characters: '{task_id}'"
            )));
        }

        if self.workspace.as_os_str().is_empty() {
            return Err(RouterError::InvalidReceipt(
                "Receipt 'workspace' path cannot be empty".to_string(),
            ));
        }

        // Validate compilation check consistency
        let comp_status = self.checks.compilation.status.to_ascii_lowercase();
        if comp_status != "passed" && comp_status != "failed" {
            return Err(RouterError::InvalidReceipt(format!(
                "Invalid compilation check status: '{}'. Expected 'passed' or 'failed'",
                self.checks.compilation.status
            )));
        }
        if comp_status == "passed" && self.checks.compilation.exit_code != 0 {
            return Err(RouterError::InvalidReceipt(format!(
                "Compilation marked 'passed' but exit_code is non-zero ({})",
                self.checks.compilation.exit_code
            )));
        }

        // Validate tests check consistency
        let test_status = self.checks.tests.status.to_ascii_lowercase();
        if test_status != "passed" && test_status != "failed" {
            return Err(RouterError::InvalidReceipt(format!(
                "Invalid tests check status: '{}'. Expected 'passed' or 'failed'",
                self.checks.tests.status
            )));
        }
        if test_status == "passed" && self.checks.tests.failed > 0 {
            return Err(RouterError::InvalidReceipt(format!(
                "Tests marked 'passed' but failed count is non-zero ({})",
                self.checks.tests.failed
            )));
        }
        if self.checks.tests.failed > self.checks.tests.total {
            return Err(RouterError::InvalidReceipt(format!(
                "Tests failed count ({}) exceeds total test count ({})",
                self.checks.tests.failed, self.checks.tests.total
            )));
        }

        // Validate linter check consistency
        let lint_status = self.checks.linter.status.to_ascii_lowercase();
        if lint_status != "passed" && lint_status != "failed" {
            return Err(RouterError::InvalidReceipt(format!(
                "Invalid linter check status: '{}'. Expected 'passed' or 'failed'",
                self.checks.linter.status
            )));
        }
        if lint_status == "passed" && self.checks.linter.warnings > 0 {
            return Err(RouterError::InvalidReceipt(format!(
                "Linter marked 'passed' but warnings count is non-zero ({})",
                self.checks.linter.warnings
            )));
        }

        // Validate scope guard check consistency
        let scope_status = self.checks.scope_guard.status.to_ascii_lowercase();
        if scope_status != "passed" && scope_status != "failed" {
            return Err(RouterError::InvalidReceipt(format!(
                "Invalid scope_guard check status: '{}'. Expected 'passed' or 'failed'",
                self.checks.scope_guard.status
            )));
        }

        // Validate timestamp format if provided
        if let Some(ref ts) = self.artifacts.receipt_timestamp {
            DateTime::parse_from_rfc3339(ts).map_err(|e| {
                RouterError::InvalidReceipt(format!(
                    "Invalid receipt_timestamp RFC-3339 format '{ts}': {e}"
                ))
            })?;
        }

        // Validate git_patch_sha256 format if provided
        if let Some(ref sha) = self.artifacts.git_patch_sha256 {
            let trimmed = sha.trim();
            if trimmed.len() != 64 || hex::decode(trimmed).is_err() {
                return Err(RouterError::InvalidReceipt(format!(
                    "Invalid git_patch_sha256 format: expected 64 hexadecimal characters, got '{sha}'"
                )));
            }
        }

        Ok(())
    }

    /// Cryptographically verifies the provided git patch bytes against `artifacts.git_patch_sha256`.
    ///
    /// If `artifacts.git_patch_sha256` is `None`, returns `Ok(true)` (no digest specified).
    /// If specified, hashes `patch_bytes` using SHA-256 and compares constant-time / case-insensitively.
    pub fn verify_patch_sha256(&self, patch_bytes: &[u8]) -> Result<bool, RouterError> {
        let expected = match &self.artifacts.git_patch_sha256 {
            Some(sha) => sha.trim(),
            None => return Ok(true),
        };

        if expected.len() != 64 {
            return Err(RouterError::InvalidReceipt(
                "Invalid git_patch_sha256: expected 64 hex characters".to_string(),
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(patch_bytes);
        let actual_bytes = hasher.finalize();
        let actual_hex = hex::encode(actual_bytes);

        Ok(actual_hex.eq_ignore_ascii_case(expected))
    }

    /// Returns `true` if the verdict is `Success` and all checks passed.
    pub fn is_passing(&self) -> bool {
        self.verdict.is_success() && self.checks.is_all_passed()
    }

    /// Produces a human-readable and serializable summary.
    pub fn summary(&self) -> ReceiptSummary {
        let checks_summary = format!(
            "compilation: {} (exit {}), tests: {} ({}/{} failed), linter: {} ({} warnings), scope_guard: {} ({} files modified)",
            self.checks.compilation.status,
            self.checks.compilation.exit_code,
            self.checks.tests.status,
            self.checks.tests.failed,
            self.checks.tests.total,
            self.checks.linter.status,
            self.checks.linter.warnings,
            self.checks.scope_guard.status,
            self.checks.scope_guard.modified_files.len()
        );

        ReceiptSummary {
            task_id: self.task_id.clone(),
            verdict: self.verdict,
            is_passing: self.is_passing(),
            workspace: self.workspace.clone(),
            checks_summary,
            receipt_timestamp: self.artifacts.receipt_timestamp.clone(),
            git_patch_sha256: self.artifacts.git_patch_sha256.clone(),
        }
    }
}

/// Persistent storage repository for execution receipts.
#[derive(Debug, Clone)]
pub struct ReceiptStore {
    base_dir: PathBuf,
}

impl Default for ReceiptStore {
    fn default() -> Self {
        Self {
            base_dir: resolve_receipts_dir(None),
        }
    }
}

impl ReceiptStore {
    /// Creates a new `ReceiptStore` rooted at `base_dir`.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Default directory path for receipts storage resolved via `resolve_receipts_dir(None)`.
    pub fn default_dir() -> PathBuf {
        resolve_receipts_dir(None)
    }

    /// Returns the underlying base directory.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Resolves the filesystem path for a given task ID.
    fn resolve_path(&self, task_id: &str) -> Result<PathBuf, RouterError> {
        let trimmed = task_id.trim();
        if trimmed.is_empty() {
            return Err(RouterError::InvalidReceipt(
                "Task ID cannot be empty".to_string(),
            ));
        }

        if trimmed.contains("..")
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains('\0')
        {
            return Err(RouterError::InvalidReceipt(format!(
                "Task ID contains invalid path traversal tokens: '{task_id}'"
            )));
        }

        let clean_id = trimmed.strip_suffix(".json").unwrap_or(trimmed);
        Ok(self.base_dir.join(format!("{clean_id}.json")))
    }

    /// Validates and saves an execution receipt under `<base_dir>/<task_id>.json`.
    pub fn save(&self, receipt: &ExecutionReceipt) -> Result<PathBuf, RouterError> {
        receipt.validate()?;
        fs::create_dir_all(&self.base_dir)?;

        let target_path = self.resolve_path(&receipt.task_id)?;
        let json_data = serde_json::to_string_pretty(receipt)?;
        fs::write(&target_path, json_data)?;

        Ok(target_path)
    }

    /// Retrieves a stored execution receipt by task ID.
    pub fn get(&self, task_id: &str) -> Result<Option<ExecutionReceipt>, RouterError> {
        let target_path = self.resolve_path(task_id)?;
        if !target_path.is_file() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&target_path)?;
        let receipt: ExecutionReceipt = serde_json::from_str(&contents)?;
        receipt.validate()?;
        Ok(Some(receipt))
    }

    /// Lists all stored execution receipts, sorted deterministically.
    pub fn list(&self) -> Result<Vec<ExecutionReceipt>, RouterError> {
        if !self.base_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut receipts = Vec::new();
        let entries = fs::read_dir(&self.base_dir)?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(contents) = fs::read_to_string(&path) {
                    if let Ok(receipt) = serde_json::from_str::<ExecutionReceipt>(&contents) {
                        receipts.push(receipt);
                    }
                }
            }
        }

        // Sort deterministically: timestamp descending (if present), tie-break task_id ascending
        receipts.sort_by(|a, b| {
            let time_a = a.artifacts.receipt_timestamp.as_deref().unwrap_or("");
            let time_b = b.artifacts.receipt_timestamp.as_deref().unwrap_or("");
            time_b.cmp(time_a).then_with(|| a.task_id.cmp(&b.task_id))
        });

        Ok(receipts)
    }

    /// Deletes a stored execution receipt if present.
    pub fn delete(&self, task_id: &str) -> Result<bool, RouterError> {
        let target_path = self.resolve_path(task_id)?;
        if target_path.is_file() {
            fs::remove_file(target_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
