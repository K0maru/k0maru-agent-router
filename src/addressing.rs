//! Deterministic Physical Addressing Engine for k0maru-agent-router.
//!
//! Provides canonical filesystem path normalization and session resolution based
//! strictly on filesystem working directory (CWD) equality without relying on
//! human-assigned pane or agent names.

use crate::driver::{LiveSession, SessionStatus};
use crate::error::RouterError;
use std::path::{Path, PathBuf};

/// Normalizes and canonicalizes a workspace path, resolving all symlinks, relative
/// segments (`.`, `..`), and redundant trailing slashes.
///
/// # Truth Invariant
/// Fails fast with `RouterError::WorkspaceNotFound` if the path does not exist on disk,
/// is empty or whitespace-only, or points to a non-directory file.
pub fn normalize_workspace_path(path: impl AsRef<Path>) -> Result<PathBuf, RouterError> {
    let p = path.as_ref();

    // Reject empty or whitespace-only paths
    if p.as_os_str().is_empty() || p.to_string_lossy().trim().is_empty() {
        return Err(RouterError::WorkspaceNotFound(p.to_path_buf()));
    }

    match std::fs::canonicalize(p) {
        Ok(canon) => {
            if !canon.is_dir() {
                return Err(RouterError::WorkspaceNotFound(p.to_path_buf()));
            }
            Ok(canon)
        }
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                || err.kind() == std::io::ErrorKind::NotADirectory =>
        {
            Err(RouterError::WorkspaceNotFound(p.to_path_buf()))
        }
        Err(_) => Err(RouterError::WorkspaceNotFound(p.to_path_buf())),
    }
}

/// Compares two workspace paths to determine if they refer to the same physical directory,
/// taking into account canonicalization, symlinks (e.g. `/var` vs `/private/var`),
/// relative path resolution against CWD, and trailing slashes.
pub fn is_same_workspace(p1: impl AsRef<Path>, p2: impl AsRef<Path>) -> bool {
    let p1 = p1.as_ref();
    let p2 = p2.as_ref();

    if p1 == p2 {
        return true;
    }

    if let (Ok(c1), Ok(c2)) = (std::fs::canonicalize(p1), std::fs::canonicalize(p2)) {
        if c1 == c2 {
            return true;
        }
    }

    let s1 = p1.to_string_lossy().trim_end_matches('/').to_string();
    let s2 = p2.to_string_lossy().trim_end_matches('/').to_string();
    if s1 == s2 {
        return true;
    }

    // Try resolving relative paths against current directory
    if let Ok(cwd) = std::env::current_dir() {
        let abs1 = if p1.is_relative() {
            cwd.join(p1)
        } else {
            p1.to_path_buf()
        };
        let abs2 = if p2.is_relative() {
            cwd.join(p2)
        } else {
            p2.to_path_buf()
        };
        if let (Ok(c1), Ok(c2)) = (std::fs::canonicalize(&abs1), std::fs::canonicalize(&abs2)) {
            if c1 == c2 {
                return true;
            }
        }
    }

    false
}

/// Resolution outcome when matching a target workspace against active multiplexer sessions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionResolution {
    /// An active multiplexer session already exists in the target workspace.
    HotSession(LiveSession),
    /// No live session exists for the target workspace; a new pane/session must be spawned.
    ColdSpawn { canonical_path: PathBuf },
}

impl SessionResolution {
    /// Returns `true` if a live session was resolved.
    pub fn is_hot(&self) -> bool {
        matches!(self, Self::HotSession(_))
    }

    /// Returns `true` if a cold spawn is required.
    pub fn is_cold(&self) -> bool {
        matches!(self, Self::ColdSpawn { .. })
    }

    /// Returns a reference to the inner `LiveSession` if hot.
    pub fn session(&self) -> Option<&LiveSession> {
        match self {
            Self::HotSession(ref s) => Some(s),
            Self::ColdSpawn { .. } => None,
        }
    }

    /// Returns the target canonical path.
    pub fn canonical_path(&self) -> &Path {
        match self {
            Self::HotSession(ref s) => &s.cwd,
            Self::ColdSpawn { ref canonical_path } => canonical_path,
        }
    }
}

/// Resolves a target workspace against a slice of live sessions.
///
/// # Prioritization Rules
/// 1. First, normalizes `target_workspace`. If non-existent or invalid, fails fast
///    with `RouterError::WorkspaceNotFound`.
/// 2. Matches live sessions whose canonical CWD equals `canonical_target`. Stale live sessions
///    whose working directories have been deleted are ignored gracefully.
/// 3. If matching sessions exist:
///    - Prefers a session with `SessionStatus::Idle` (ready to accept commands).
///    - Otherwise, falls back to the first matching `SessionStatus::Working` session.
///    - Returns `Ok(SessionResolution::HotSession(selected))`.
/// 4. If no matching session exists, returns `Ok(SessionResolution::ColdSpawn { canonical_path })`.
pub fn resolve_session(
    target_workspace: impl AsRef<Path>,
    live_sessions: &[LiveSession],
) -> Result<SessionResolution, RouterError> {
    let canonical_target = normalize_workspace_path(target_workspace)?;

    let mut matching_idle = Vec::new();
    let mut matching_working = Vec::new();

    for session in live_sessions {
        let session_canon = match std::fs::canonicalize(&session.cwd) {
            Ok(p) => p,
            Err(_) => session.cwd.clone(),
        };

        if session_canon == canonical_target {
            match session.status {
                SessionStatus::Idle => matching_idle.push(session.clone()),
                SessionStatus::Working => matching_working.push(session.clone()),
            }
        }
    }

    if let Some(idle) = matching_idle.into_iter().next() {
        Ok(SessionResolution::HotSession(idle))
    } else if let Some(working) = matching_working.into_iter().next() {
        Ok(SessionResolution::HotSession(working))
    } else {
        Ok(SessionResolution::ColdSpawn {
            canonical_path: canonical_target,
        })
    }
}

/// Determines if a target network node identifier corresponds to the local machine.
///
/// Returns `true` if `node` is `None`, empty, or matches common loopback/local aliases
/// (`"local"`, `"localhost"`, `"127.0.0.1"`, `"::1"`) or the machine's hostname.
pub fn is_local_node(node: Option<&str>) -> bool {
    let Some(n) = node else { return true };
    let clean = n.trim().to_lowercase();
    if clean.is_empty()
        || clean == "local"
        || clean == "localhost"
        || clean == "127.0.0.1"
        || clean == "::1"
    {
        return true;
    }

    if let Ok(host) = std::env::var("HOSTNAME") {
        if host.trim().to_lowercase() == clean {
            return true;
        }
    }

    if let Ok(output) = std::process::Command::new("hostname").output() {
        if output.status.success() {
            let sys_host = String::from_utf8_lossy(&output.stdout);
            if sys_host.trim().to_lowercase() == clean {
                return true;
            }
        }
    }

    false
}

/// Resolves a target workspace against a slice of live sessions on a remote host
/// without performing local filesystem canonicalization checks.
pub fn resolve_remote_session(
    target_workspace: impl AsRef<Path>,
    live_sessions: &[LiveSession],
) -> SessionResolution {
    let target_path = target_workspace.as_ref();
    let norm_target = target_path
        .to_string_lossy()
        .trim_end_matches('/')
        .to_string();

    let mut idle_match = None;
    let mut working_match = None;

    for session in live_sessions {
        let s_path = session
            .cwd
            .to_string_lossy()
            .trim_end_matches('/')
            .to_string();
        if s_path == norm_target
            || s_path.ends_with(&format!("/{norm_target}"))
            || norm_target.ends_with(&format!("/{s_path}"))
        {
            if session.status == SessionStatus::Idle && idle_match.is_none() {
                idle_match = Some(session.clone());
            } else if working_match.is_none() {
                working_match = Some(session.clone());
            }
        }
    }

    if let Some(session) = idle_match.or(working_match) {
        SessionResolution::HotSession(session)
    } else {
        SessionResolution::ColdSpawn {
            canonical_path: target_path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_normalize_existing_dir() {
        let dir = tempdir().unwrap();
        let normalized = normalize_workspace_path(dir.path()).unwrap();
        assert_eq!(normalized, std::fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn test_normalize_relative_segments() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();

        let rel = sub.join("..").join("b").join(".");
        let normalized = normalize_workspace_path(&rel).unwrap();
        assert_eq!(normalized, std::fs::canonicalize(&sub).unwrap());
    }

    #[test]
    fn test_normalize_trailing_slashes() {
        let dir = tempdir().unwrap();
        let trailing = format!("{}///", dir.path().display());
        let normalized = normalize_workspace_path(&trailing).unwrap();
        assert_eq!(normalized, std::fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn test_normalize_symlinks() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("real_dir");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link_dir");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let normalized = normalize_workspace_path(&link).unwrap();
            assert_eq!(normalized, std::fs::canonicalize(&real).unwrap());
        }
    }

    #[test]
    fn test_normalize_broken_symlink_fails() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("deleted_dir");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("broken_link");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).unwrap();
            std::fs::remove_dir(&target).unwrap();

            let err = normalize_workspace_path(&link).unwrap_err();
            assert_eq!(err, RouterError::WorkspaceNotFound(link));
        }
    }

    #[test]
    fn test_normalize_file_fails() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "hello").unwrap();

        let err = normalize_workspace_path(&file).unwrap_err();
        assert_eq!(err, RouterError::WorkspaceNotFound(file));
    }

    #[test]
    fn test_normalize_empty_and_whitespace_fails() {
        let err1 = normalize_workspace_path("").unwrap_err();
        assert_eq!(err1, RouterError::WorkspaceNotFound(PathBuf::from("")));

        let err2 = normalize_workspace_path("   ").unwrap_err();
        assert_eq!(err2, RouterError::WorkspaceNotFound(PathBuf::from("   ")));
    }

    #[test]
    fn test_resolve_cold_spawn_when_empty() {
        let dir = tempdir().unwrap();
        let canon = std::fs::canonicalize(dir.path()).unwrap();
        let res = resolve_session(dir.path(), &[]).unwrap();
        assert_eq!(
            res,
            SessionResolution::ColdSpawn {
                canonical_path: canon.clone()
            }
        );
        assert!(res.is_cold());
        assert!(!res.is_hot());
        assert_eq!(res.canonical_path(), &canon);
        assert!(res.session().is_none());
    }

    #[test]
    fn test_resolve_hot_session_idle() {
        let dir = tempdir().unwrap();
        let canon = std::fs::canonicalize(dir.path()).unwrap();
        let session =
            LiveSession::new("%1", canon.clone(), SessionStatus::Idle, Some("zsh".into()));

        let res = resolve_session(dir.path(), std::slice::from_ref(&session)).unwrap();
        assert_eq!(res, SessionResolution::HotSession(session));
        assert!(res.is_hot());
        assert!(!res.is_cold());
        assert_eq!(res.session().unwrap().pane_id, "%1");
        assert_eq!(res.canonical_path(), &canon);
    }

    #[test]
    fn test_resolve_prefers_idle_over_working() {
        let dir = tempdir().unwrap();
        let canon = std::fs::canonicalize(dir.path()).unwrap();
        let working = LiveSession::new(
            "%1",
            canon.clone(),
            SessionStatus::Working,
            Some("cargo".into()),
        );
        let idle = LiveSession::new("%2", canon, SessionStatus::Idle, Some("bash".into()));

        // Put working first in list; idle should be preferred
        let res = resolve_session(dir.path(), &[working, idle.clone()]).unwrap();
        assert_eq!(res, SessionResolution::HotSession(idle));
    }

    #[test]
    fn test_resolve_falls_back_to_working() {
        let dir = tempdir().unwrap();
        let canon = std::fs::canonicalize(dir.path()).unwrap();
        let working1 = LiveSession::new(
            "%1",
            canon.clone(),
            SessionStatus::Working,
            Some("cargo".into()),
        );
        let working2 = LiveSession::new("%2", canon, SessionStatus::Working, Some("python".into()));

        let res = resolve_session(dir.path(), &[working1.clone(), working2]).unwrap();
        assert_eq!(res, SessionResolution::HotSession(working1));
    }

    #[test]
    fn test_is_local_node_variations() {
        assert!(is_local_node(None));
        assert!(is_local_node(Some("")));
        assert!(is_local_node(Some("local")));
        assert!(is_local_node(Some("LOCAL")));
        assert!(is_local_node(Some("localhost")));
        assert!(is_local_node(Some("127.0.0.1")));
        assert!(is_local_node(Some("::1")));

        assert!(!is_local_node(Some("company-linux")));
        assert!(!is_local_node(Some("user@100.88.22.33")));
        assert!(!is_local_node(Some("remote-host.tailscale.net")));
    }

    #[test]
    fn test_resolve_remote_session_matches_cwd() {
        let remote_ws = Path::new("/home/k0maru3/work/backend");
        let session = LiveSession::new(
            "%10",
            PathBuf::from("/home/k0maru3/work/backend"),
            SessionStatus::Idle,
            Some("bash".into()),
        );

        let res = resolve_remote_session(remote_ws, std::slice::from_ref(&session));
        assert_eq!(res, SessionResolution::HotSession(session));

        let other_ws = Path::new("/var/other/service");
        let res2 = resolve_remote_session(other_ws, &[]);
        assert!(res2.is_cold());
        assert_eq!(res2.canonical_path(), other_ws);
    }
}
