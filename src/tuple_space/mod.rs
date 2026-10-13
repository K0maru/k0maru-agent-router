//! Linda Tuple Space Engine for K0maru-Harness.
//!
//! Provides associative shared-memory primitives (`out`, `in`, `rd`, `list`)
//! operating on local filesystem buses with POSIX-atomic file locks and zero daemons.

pub mod tuple;

use std::fs;
use std::path::{Path, PathBuf};

pub use tuple::{Tuple, TuplePattern};

use crate::error::RouterError;
use crate::receipt::DEFAULT_K0MARU_DIR;

/// Environment variable controlling global tuple space storage directory.
pub const ENV_K0MARU_TUPLE_DIR: &str = "K0MARU_TUPLE_DIR";

/// Default subfolder name for tuple space storage.
pub const DEFAULT_TUPLES_DIR: &str = "tuples";

/// Resolves the canonical tuple space storage directory based on standard precedence:
/// 1. Priority 1: Explicit override path if supplied
/// 2. Priority 2: Environment variable `K0MARU_TUPLE_DIR`
/// 3. Priority 3: Default home directory `~/.k0maru/tuples` (fallback to `.k0maru/tuples`)
pub fn resolve_tuples_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }

    if let Ok(env_val) = std::env::var(ENV_K0MARU_TUPLE_DIR) {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if let Some(home) = dirs::home_dir() {
        home.join(DEFAULT_K0MARU_DIR).join(DEFAULT_TUPLES_DIR)
    } else {
        PathBuf::from(DEFAULT_K0MARU_DIR).join(DEFAULT_TUPLES_DIR)
    }
}

/// Linda Tuple Space Engine providing atomic shared-memory operations.
#[derive(Debug, Clone)]
pub struct TupleSpaceEngine {
    tuple_dir: PathBuf,
}

impl TupleSpaceEngine {
    /// Creates a new `TupleSpaceEngine` backed by the given directory.
    pub fn new(tuple_dir: impl Into<PathBuf>) -> Self {
        Self {
            tuple_dir: tuple_dir.into(),
        }
    }

    /// Creates a new `TupleSpaceEngine` using the default global directory.
    pub fn with_default_dir() -> Self {
        Self::new(resolve_tuples_dir(None))
    }

    /// Returns a reference to the underlying tuple space directory.
    pub fn tuple_dir(&self) -> &Path {
        &self.tuple_dir
    }

    /// Writes a tuple atomically into the space (`out` primitive).
    pub fn out(&self, mut tuple: Tuple) -> Result<Tuple, RouterError> {
        if tuple.id.trim().is_empty() {
            tuple.id = format!("tup_{}", uuid::Uuid::new_v4().simple());
        }
        if tuple.created_at.trim().is_empty() {
            tuple.created_at = chrono::Utc::now().to_rfc3339();
        }

        // Validate tuple ID to prevent directory traversal
        if tuple.id.contains('/') || tuple.id.contains('\\') || tuple.id.contains("..") {
            return Err(RouterError::TupleSpaceError(format!(
                "Invalid tuple ID '{}': must not contain path traversal characters",
                tuple.id
            )));
        }

        fs::create_dir_all(&self.tuple_dir).map_err(|e| {
            RouterError::TupleSpaceError(format!(
                "Failed to create tuple directory '{}': {e}",
                self.tuple_dir.display()
            ))
        })?;

        let target_file = self.tuple_dir.join(format!("{}.json", tuple.id));
        let tmp_file = self.tuple_dir.join(format!(
            "{}.tmp.{}",
            tuple.id,
            uuid::Uuid::new_v4().simple()
        ));

        let serialized = serde_json::to_string_pretty(&tuple)
            .map_err(|e| RouterError::TupleSpaceError(format!("Failed to serialize tuple: {e}")))?;

        fs::write(&tmp_file, serialized).map_err(|e| {
            RouterError::TupleSpaceError(format!(
                "Failed to write temporary tuple file '{}': {e}",
                tmp_file.display()
            ))
        })?;

        fs::rename(&tmp_file, &target_file).map_err(|e| {
            let _ = fs::remove_file(&tmp_file);
            RouterError::TupleSpaceError(format!(
                "Failed to atomically persist tuple '{}': {e}",
                target_file.display()
            ))
        })?;

        Ok(tuple)
    }

    /// Non-destructively reads the first matching tuple (`rd` primitive).
    pub fn rd(&self, pattern: &TuplePattern) -> Result<Option<Tuple>, RouterError> {
        let candidates = self.scan_candidates()?;
        for path in candidates {
            if let Some(tuple) = self.read_tuple_from_file(&path)? {
                if !tuple.is_expired() && pattern.matches(&tuple) {
                    return Ok(Some(tuple));
                }
            }
        }
        Ok(None)
    }

    /// Atomically finds, claims, and removes a matching tuple (`in` primitive).
    pub fn take(&self, pattern: &TuplePattern) -> Result<Option<Tuple>, RouterError> {
        let candidates = self.scan_candidates()?;
        for path in candidates {
            // First check if it matches without claiming
            if let Some(tuple) = self.read_tuple_from_file(&path)? {
                if tuple.is_expired() {
                    continue;
                }
                if pattern.matches(&tuple) {
                    // Attempt atomic claim via rename
                    let claim_file =
                        path.with_extension(format!("claimed.{}", uuid::Uuid::new_v4().simple()));
                    if fs::rename(&path, &claim_file).is_ok() {
                        // Successfully claimed! Read and parse from claim file to be 100% sure
                        let content = fs::read_to_string(&claim_file).map_err(|e| {
                            RouterError::TupleSpaceError(format!(
                                "Failed to read claimed tuple: {e}"
                            ))
                        })?;
                        let _ = fs::remove_file(&claim_file);
                        let claimed_tuple: Tuple = serde_json::from_str(&content).map_err(|e| {
                            RouterError::TupleSpaceError(format!(
                                "Failed to parse claimed tuple: {e}"
                            ))
                        })?;
                        return Ok(Some(claimed_tuple));
                    }
                    // If rename failed, another process claimed it concurrently; continue scanning
                }
            }
        }
        Ok(None)
    }

    /// Alias for `take` to match formal Linda `in` primitive without keyword collision.
    pub fn r#in(&self, pattern: &TuplePattern) -> Result<Option<Tuple>, RouterError> {
        self.take(pattern)
    }

    /// Non-destructively reads the first matching tuple, waiting up to `timeout` if not immediately available.
    pub fn rd_wait(
        &self,
        pattern: &TuplePattern,
        timeout: std::time::Duration,
    ) -> Result<Option<Tuple>, RouterError> {
        let start = std::time::Instant::now();
        let mut interval_ms = 10u64;

        loop {
            if let Some(tuple) = self.rd(pattern)? {
                return Ok(Some(tuple));
            }

            if start.elapsed() >= timeout {
                return Ok(None);
            }

            let remaining = timeout.saturating_sub(start.elapsed());
            let sleep_duration = std::time::Duration::from_millis(interval_ms).min(remaining);
            if sleep_duration.is_zero() {
                return Ok(None);
            }
            std::thread::sleep(sleep_duration);
            interval_ms = (interval_ms * 2).min(100);
        }
    }

    /// Atomically claims and removes a matching tuple, waiting up to `timeout` if not immediately available.
    pub fn take_wait(
        &self,
        pattern: &TuplePattern,
        timeout: std::time::Duration,
    ) -> Result<Option<Tuple>, RouterError> {
        let start = std::time::Instant::now();
        let mut interval_ms = 10u64;

        loop {
            if let Some(tuple) = self.take(pattern)? {
                return Ok(Some(tuple));
            }

            if start.elapsed() >= timeout {
                return Ok(None);
            }

            let remaining = timeout.saturating_sub(start.elapsed());
            let sleep_duration = std::time::Duration::from_millis(interval_ms).min(remaining);
            if sleep_duration.is_zero() {
                return Ok(None);
            }
            std::thread::sleep(sleep_duration);
            interval_ms = (interval_ms * 2).min(100);
        }
    }

    /// Alias for `take_wait` to match formal Linda `in` primitive with reactive wait.
    pub fn r#in_wait(
        &self,
        pattern: &TuplePattern,
        timeout: std::time::Duration,
    ) -> Result<Option<Tuple>, RouterError> {
        self.take_wait(pattern, timeout)
    }

    /// Lists all non-expired tuples matching the given pattern without consuming them.
    pub fn list(&self, pattern: &TuplePattern) -> Result<Vec<Tuple>, RouterError> {
        let candidates = self.scan_candidates()?;
        let mut matched = Vec::new();
        for path in candidates {
            if let Some(tuple) = self.read_tuple_from_file(&path)? {
                if !tuple.is_expired() && pattern.matches(&tuple) {
                    matched.push(tuple);
                }
            }
        }
        // Sort deterministically by created_at ascending, then id
        matched.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(matched)
    }

    /// Scans the tuple directory for all candidate JSON files.
    fn scan_candidates(&self) -> Result<Vec<PathBuf>, RouterError> {
        if !self.tuple_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&self.tuple_dir).map_err(|e| {
            RouterError::TupleSpaceError(format!(
                "Failed to read tuple directory '{}': {e}",
                self.tuple_dir.display()
            ))
        })?;

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "json" {
                        entries.push(path);
                    }
                }
            }
        }

        entries.sort();
        Ok(entries)
    }

    /// Reads and deserializes a tuple from a file, returning None if file disappeared.
    fn read_tuple_from_file(&self, path: &Path) -> Result<Option<Tuple>, RouterError> {
        match fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<Tuple>(&content) {
                Ok(tuple) => Ok(Some(tuple)),
                Err(e) => Err(RouterError::TupleSpaceError(format!(
                    "Failed to parse tuple file '{}': {e}",
                    path.display()
                ))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(RouterError::TupleSpaceError(format!(
                "Failed to read tuple file '{}': {e}",
                path.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn test_basic_out_rd_list_and_take() {
        let temp = tempdir().unwrap();
        let engine = TupleSpaceEngine::new(temp.path());

        let t1 = Tuple::new("TASK_READY", "AUTH-SRV", "t1", "!δ:0.1|AUTH|src#1");
        let t1_id = t1.id.clone();
        let saved = engine.out(t1).expect("Failed to out t1");
        assert_eq!(saved.id, t1_id);

        // Non-destructive read
        let pat = TuplePattern::new().with_kind("TASK_READY");
        let read = engine.rd(&pat).expect("rd failed");
        assert!(read.is_some());
        assert_eq!(read.as_ref().unwrap().id, t1_id);

        // List
        let list = engine.list(&pat).expect("list failed");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, t1_id);

        // Destructive take (in)
        let taken = engine.take(&pat).expect("take failed");
        assert!(taken.is_some());
        assert_eq!(taken.as_ref().unwrap().id, t1_id);

        // After take, space is empty
        let read_after = engine.rd(&pat).expect("rd after failed");
        assert!(read_after.is_none());
        let list_after = engine.list(&pat).expect("list after failed");
        assert!(list_after.is_empty());
    }

    #[test]
    fn test_pattern_matching_dimensions() {
        let temp = tempdir().unwrap();
        let engine = TupleSpaceEngine::new(temp.path());

        engine
            .out(Tuple::new("KIND_A", "DOM_1", "TASK_X", "p1"))
            .unwrap();
        engine
            .out(Tuple::new("KIND_A", "DOM_2", "TASK_Y", "p2"))
            .unwrap();
        engine
            .out(Tuple::new("KIND_B", "DOM_1", "TASK_Z", "p3"))
            .unwrap();

        // Match by kind
        let list_kind_a = engine
            .list(&TuplePattern::new().with_kind("KIND_A"))
            .unwrap();
        assert_eq!(list_kind_a.len(), 2);

        // Match by domain
        let list_dom_1 = engine
            .list(&TuplePattern::new().with_domain("DOM_1"))
            .unwrap();
        assert_eq!(list_dom_1.len(), 2);

        // Match by task_id
        let list_task_x = engine
            .list(&TuplePattern::new().with_task_id("TASK_X"))
            .unwrap();
        assert_eq!(list_task_x.len(), 1);
        assert_eq!(list_task_x[0].payload, "p1");

        // Match by combined kind + domain
        let list_combined = engine
            .list(&TuplePattern::new().with_kind("KIND_A").with_domain("DOM_2"))
            .unwrap();
        assert_eq!(list_combined.len(), 1);
        assert_eq!(list_combined[0].task_id, "TASK_Y");

        // Wildcard match
        let list_all = engine.list(&TuplePattern::any()).unwrap();
        assert_eq!(list_all.len(), 3);
    }

    #[test]
    fn test_ttl_expiration() {
        let temp = tempdir().unwrap();
        let engine = TupleSpaceEngine::new(temp.path());

        // Create an expired tuple (1 hour in the past with 10s TTL)
        let past_time = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let expired_tuple = Tuple::new("TASK_EXP", "DOM", "t_exp", "payload")
            .with_created_at(past_time)
            .with_ttl(10);
        engine.out(expired_tuple).unwrap();

        // Fresh tuple
        let fresh_tuple = Tuple::new("TASK_FRESH", "DOM", "t_fresh", "payload").with_ttl(3600);
        engine.out(fresh_tuple).unwrap();

        let pat_exp = TuplePattern::new().with_kind("TASK_EXP");
        assert!(engine.rd(&pat_exp).unwrap().is_none());
        assert!(engine.take(&pat_exp).unwrap().is_none());
        assert!(engine.list(&pat_exp).unwrap().is_empty());

        let pat_fresh = TuplePattern::new().with_kind("TASK_FRESH");
        assert!(engine.rd(&pat_fresh).unwrap().is_some());
        assert_eq!(engine.list(&pat_fresh).unwrap().len(), 1);
    }

    #[test]
    fn test_concurrent_producers_and_consumers() {
        let temp = tempdir().unwrap();
        let engine = Arc::new(TupleSpaceEngine::new(temp.path()));

        let tuple_count = 30;

        // Produce tuples
        for i in 0..tuple_count {
            engine
                .out(Tuple::new(
                    "CONCURRENT_TASK",
                    "FABRIC",
                    format!("task_{i}"),
                    format!("payload_{i}"),
                ))
                .unwrap();
        }

        // Spawn 5 consumer threads competing to claim tuples
        let mut handles = vec![];
        for _ in 0..5 {
            let engine_clone = Arc::clone(&engine);
            let handle = thread::spawn(move || {
                let pat = TuplePattern::new().with_kind("CONCURRENT_TASK");
                let mut claimed = Vec::new();
                while let Ok(Some(tuple)) = engine_clone.take(&pat) {
                    claimed.push(tuple.task_id);
                }
                claimed
            });
            handles.push(handle);
        }

        let mut all_claimed = Vec::new();
        for h in handles {
            let claimed = h.join().unwrap();
            all_claimed.extend(claimed);
        }

        // Verify exactly 30 tuples claimed with zero duplication
        assert_eq!(all_claimed.len(), tuple_count);
        all_claimed.sort();
        all_claimed.dedup();
        assert_eq!(all_claimed.len(), tuple_count);

        // Ensure space is empty now
        let list_remaining = engine
            .list(&TuplePattern::new().with_kind("CONCURRENT_TASK"))
            .unwrap();
        assert!(list_remaining.is_empty());
    }

    #[test]
    fn test_reactive_take_wait_success() {
        let temp = tempdir().unwrap();
        let engine = Arc::new(TupleSpaceEngine::new(temp.path()));

        let engine_clone = Arc::clone(&engine);
        thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(60));
            engine_clone
                .out(Tuple::new(
                    "REACTIVE_KIND",
                    "ROUTER",
                    "task_r1",
                    "payload_r1",
                ))
                .unwrap();
        });

        let pat = TuplePattern::new().with_kind("REACTIVE_KIND");
        let claimed = engine
            .take_wait(&pat, std::time::Duration::from_millis(500))
            .expect("take_wait failed")
            .expect("Should have claimed tuple reactively");

        assert_eq!(claimed.task_id, "task_r1");
    }

    #[test]
    fn test_reactive_take_wait_timeout() {
        let temp = tempdir().unwrap();
        let engine = TupleSpaceEngine::new(temp.path());

        let pat = TuplePattern::new().with_kind("NON_EXISTENT");
        let start = std::time::Instant::now();
        let claimed = engine
            .take_wait(&pat, std::time::Duration::from_millis(50))
            .expect("take_wait failed");

        assert!(claimed.is_none());
        assert!(start.elapsed() >= std::time::Duration::from_millis(45));
    }

    #[test]
    fn test_reactive_rd_wait_success() {
        let temp = tempdir().unwrap();
        let engine = Arc::new(TupleSpaceEngine::new(temp.path()));

        let engine_clone = Arc::clone(&engine);
        thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(40));
            engine_clone
                .out(Tuple::new(
                    "RD_WAIT_KIND",
                    "DOM_WAIT",
                    "task_w1",
                    "payload_w1",
                ))
                .unwrap();
        });

        let pat = TuplePattern::new().with_kind("RD_WAIT_KIND");
        let found = engine
            .rd_wait(&pat, std::time::Duration::from_millis(400))
            .expect("rd_wait failed")
            .expect("Should have found tuple reactively");

        assert_eq!(found.task_id, "task_w1");
        // Verify it was NOT consumed
        assert!(engine.rd(&pat).unwrap().is_some());
    }
}
