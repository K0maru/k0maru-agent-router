//! Integration tests for Milestone M3 (R3: Deterministic Physical Addressing & Multiplexer Driver SPI).
//!
//! Covers:
//! - Suite 1: Path Normalization & Canonicalization
//! - Suite 2: Session State Resolution
//! - Suite 3: Pure Tmux Output Parser & Shell Classifier
//! - Suite 4: Hermetic Tmux Driver Execution with isolated socket

use k0maru_router::addressing::{normalize_workspace_path, resolve_session, SessionResolution};
use k0maru_router::driver::{
    classify_command_status, parse_pane_line, parse_tmux_output, LiveSession, MultiplexerDriver,
    SessionStatus, TmuxDriver,
};
use k0maru_router::error::RouterError;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

// =========================================================================
// Suite 1: Path Normalization & Canonicalization
// =========================================================================

#[test]
fn test_normalize_existing_directory() {
    let tmp = tempdir().unwrap();
    let norm = normalize_workspace_path(tmp.path()).expect("Should normalize existing dir");
    assert_eq!(norm, fs::canonicalize(tmp.path()).unwrap());
}

#[test]
fn test_normalize_relative_segments() {
    let tmp = tempdir().unwrap();
    let sub = tmp.path().join("sub");
    fs::create_dir(&sub).unwrap();

    let relative_path = sub.join("..").join("sub").join(".");
    let norm = normalize_workspace_path(&relative_path).expect("Should resolve relative path");
    assert_eq!(norm, fs::canonicalize(&sub).unwrap());
}

#[test]
fn test_normalize_trailing_slashes() {
    let tmp = tempdir().unwrap();
    let path_str = format!("{}///", tmp.path().display());
    let norm = normalize_workspace_path(&path_str).expect("Should strip trailing slashes");
    assert_eq!(norm, fs::canonicalize(tmp.path()).unwrap());
}

#[test]
fn test_normalize_symlinks() {
    let tmp = tempdir().unwrap();
    let real_dir = tmp.path().join("real_target");
    fs::create_dir(&real_dir).unwrap();
    let symlink = tmp.path().join("symlink_target");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_dir, &symlink).unwrap();

    #[cfg(unix)]
    {
        let norm = normalize_workspace_path(&symlink).expect("Should resolve symlink");
        assert_eq!(norm, fs::canonicalize(&real_dir).unwrap());
    }
}

#[test]
fn test_normalize_broken_symlink_fails_fast() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("deleted_dir");
    fs::create_dir(&target).unwrap();
    let symlink = tmp.path().join("broken_sym");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &symlink).unwrap();
        fs::remove_dir(&target).unwrap();

        let err = normalize_workspace_path(&symlink).expect_err("Broken symlink must fail");
        assert_eq!(err, RouterError::WorkspaceNotFound(symlink));
    }
}

#[test]
fn test_normalize_nonexistent_directory_fails_fast() {
    let p = PathBuf::from("/definitely/nonexistent/workspace/12345");
    let err = normalize_workspace_path(&p).expect_err("Nonexistent path must fail");
    assert_eq!(err, RouterError::WorkspaceNotFound(p));
}

#[test]
fn test_normalize_file_instead_of_directory_fails_fast() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("not_a_dir.txt");
    fs::write(&file, "content").unwrap();

    let err = normalize_workspace_path(&file).expect_err("File must fail as workspace");
    assert_eq!(err, RouterError::WorkspaceNotFound(file));
}

#[test]
fn test_normalize_empty_or_whitespace_fails_fast() {
    let err1 = normalize_workspace_path("").expect_err("Empty string must fail");
    assert_eq!(err1, RouterError::WorkspaceNotFound(PathBuf::from("")));

    let err2 = normalize_workspace_path("   ").expect_err("Whitespace must fail");
    assert_eq!(err2, RouterError::WorkspaceNotFound(PathBuf::from("   ")));
}

// =========================================================================
// Suite 2: Session State Resolution
// =========================================================================

#[test]
fn test_resolve_empty_sessions_cold_spawn() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();
    let res = resolve_session(tmp.path(), &[]).expect("Should resolve");
    assert_eq!(
        res,
        SessionResolution::ColdSpawn {
            canonical_path: canon
        }
    );
    assert!(res.is_cold());
    assert!(!res.is_hot());
}

#[test]
fn test_resolve_non_matching_sessions_cold_spawn() {
    let tmp = tempdir().unwrap();
    let other_tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();

    let live = vec![LiveSession {
        pane_id: "%0".to_string(),
        cwd: fs::canonicalize(other_tmp.path()).unwrap(),
        status: SessionStatus::Idle,
        current_command: Some("zsh".to_string()),
    }];

    let res = resolve_session(tmp.path(), &live).expect("Should resolve");
    assert_eq!(
        res,
        SessionResolution::ColdSpawn {
            canonical_path: canon
        }
    );
}

#[test]
fn test_resolve_hot_session_idle() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();

    let live = vec![LiveSession {
        pane_id: "%1".to_string(),
        cwd: canon.clone(),
        status: SessionStatus::Idle,
        current_command: Some("zsh".to_string()),
    }];

    let res = resolve_session(tmp.path(), &live).expect("Should resolve");
    assert_eq!(res, SessionResolution::HotSession(live[0].clone()));
    assert!(res.is_hot());
    assert_eq!(res.session().unwrap().pane_id, "%1");
}

#[test]
fn test_resolve_hot_session_working() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();

    let live = vec![LiveSession {
        pane_id: "%2".to_string(),
        cwd: canon.clone(),
        status: SessionStatus::Working,
        current_command: Some("cargo".to_string()),
    }];

    let res = resolve_session(tmp.path(), &live).expect("Should resolve");
    assert_eq!(res, SessionResolution::HotSession(live[0].clone()));
    assert_eq!(res.session().unwrap().status, SessionStatus::Working);
}

#[test]
fn test_resolve_priority_prefers_idle_over_working() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();

    let working_session = LiveSession {
        pane_id: "%1".to_string(),
        cwd: canon.clone(),
        status: SessionStatus::Working,
        current_command: Some("cargo".to_string()),
    };
    let idle_session = LiveSession {
        pane_id: "%2".to_string(),
        cwd: canon.clone(),
        status: SessionStatus::Idle,
        current_command: Some("zsh".to_string()),
    };

    // Working session is listed first, but Idle session must be preferred!
    let live = vec![working_session, idle_session.clone()];
    let res = resolve_session(tmp.path(), &live).expect("Should resolve");
    assert_eq!(res, SessionResolution::HotSession(idle_session));
}

#[test]
fn test_resolve_with_symlink_workspace_canonical_match() {
    let tmp = tempdir().unwrap();
    let real_dir = tmp.path().join("real");
    fs::create_dir(&real_dir).unwrap();
    let symlink = tmp.path().join("sym");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_dir, &symlink).unwrap();

    #[cfg(unix)]
    {
        let canon_real = fs::canonicalize(&real_dir).unwrap();
        let live = vec![LiveSession {
            pane_id: "%10".to_string(),
            cwd: canon_real,
            status: SessionStatus::Idle,
            current_command: Some("bash".to_string()),
        }];

        // Pass symlink as target_workspace, session has real dir
        let res = resolve_session(&symlink, &live).expect("Should match via canonical path");
        assert_eq!(res, SessionResolution::HotSession(live[0].clone()));
    }
}

#[test]
fn test_resolve_nonexistent_target_fails_fast() {
    let non_existent = PathBuf::from("/nonexistent/ws");
    let err = resolve_session(&non_existent, &[]).expect_err("Should fail");
    assert_eq!(err, RouterError::WorkspaceNotFound(non_existent));
}

#[test]
fn test_session_resolution_predicates_and_accessors() {
    let path = PathBuf::from("/test/path");
    let cold = SessionResolution::ColdSpawn {
        canonical_path: path.clone(),
    };
    assert!(cold.is_cold());
    assert!(!cold.is_hot());
    assert_eq!(cold.canonical_path(), &path);
    assert!(cold.session().is_none());

    let session = LiveSession {
        pane_id: "%5".into(),
        cwd: path.clone(),
        status: SessionStatus::Idle,
        current_command: Some("zsh".into()),
    };
    let hot = SessionResolution::HotSession(session.clone());
    assert!(hot.is_hot());
    assert!(!hot.is_cold());
    assert_eq!(hot.canonical_path(), &path);
    assert_eq!(hot.session(), Some(&session));
}

// =========================================================================
// Suite 3: Pure Tmux Output Parser & Shell Classifier
// =========================================================================

#[test]
fn test_parse_tmux_output_tsv() {
    let raw = "%0\t/private/tmp/repo\tzsh\n%1\t/private/tmp/other\tcargo\n";
    let sessions = parse_tmux_output(raw);
    assert_eq!(sessions.len(), 2);

    assert_eq!(sessions[0].pane_id, "%0");
    assert_eq!(sessions[0].cwd, PathBuf::from("/private/tmp/repo"));
    assert_eq!(sessions[0].status, SessionStatus::Idle);
    assert_eq!(sessions[0].current_command, Some("zsh".to_string()));

    assert_eq!(sessions[1].pane_id, "%1");
    assert_eq!(sessions[1].status, SessionStatus::Working);
    assert_eq!(sessions[1].current_command, Some("cargo".to_string()));
}

#[test]
fn test_parse_tmux_output_colon_delimited() {
    let raw = "%2:/var/log:bash\n";
    let sessions = parse_tmux_output(raw);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].pane_id, "%2");
    assert_eq!(sessions[0].status, SessionStatus::Idle);
}

#[test]
fn test_parse_tmux_output_path_containing_colons() {
    let raw = "%3:/tmp/repo:tag1:zsh\n";
    let session = parse_pane_line(raw.trim()).expect("Should parse path with colon");
    assert_eq!(session.pane_id, "%3");
    assert_eq!(session.cwd, PathBuf::from("/tmp/repo:tag1"));
    assert_eq!(session.status, SessionStatus::Idle);
}

#[test]
fn test_parse_tmux_output_multiline_mixed() {
    let raw = "\n  \n%0\t/tmp/ws\t-zsh\n\n%1\t/tmp/build\tpython3\n   \n";
    let sessions = parse_tmux_output(raw);
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].status, SessionStatus::Idle); // login shell -zsh
    assert_eq!(sessions[1].status, SessionStatus::Working);
}

#[test]
fn test_parse_tmux_output_empty_and_garbage_tolerance() {
    assert!(parse_pane_line("").is_none());
    assert!(parse_pane_line("   ").is_none());
    assert!(parse_pane_line("garbage without delimiter").is_none());
    assert!(parse_pane_line(":empty_pane_id").is_none());
    assert!(parse_pane_line("%1:").is_none());
    assert!(parse_tmux_output("").is_empty());
    assert!(parse_tmux_output("\n\n  \n").is_empty());
}

#[test]
fn test_session_status_classification_matrix() {
    // Idle shells
    for shell in &[
        "bash", "zsh", "sh", "fish", "ksh", "csh", "tcsh", "dash", "nu",
    ] {
        assert_eq!(
            classify_command_status(shell),
            SessionStatus::Idle,
            "Shell '{}' must be Idle",
            shell
        );
        let login = format!("-{}", shell);
        assert_eq!(
            classify_command_status(&login),
            SessionStatus::Idle,
            "Login shell '{}' must be Idle",
            login
        );
        let path = format!("/usr/bin/{}", shell);
        assert_eq!(
            classify_command_status(&path),
            SessionStatus::Idle,
            "Full path '{}' must be Idle",
            path
        );
    }

    // Working commands
    for tool in &[
        "cargo", "rustc", "sleep", "git", "vim", "nvim", "python", "node", "agy",
    ] {
        assert_eq!(
            classify_command_status(tool),
            SessionStatus::Working,
            "Tool '{}' must be Working",
            tool
        );
    }

    // Empty and None fallbacks
    assert_eq!(classify_command_status(""), SessionStatus::Working);
    assert_eq!(SessionStatus::from_command(None), SessionStatus::Working);
}

// =========================================================================
// Suite 4: Hermetic Tmux Driver Execution
// =========================================================================

#[test]
fn test_tmux_driver_is_available() {
    let driver = TmuxDriver::new();
    assert_eq!(driver.name(), "tmux");
    let which_tmux = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok();
    assert_eq!(driver.is_available(), which_tmux);
}

#[test]
fn test_tmux_driver_offline_graceful_empty_list() {
    let driver = TmuxDriver::with_socket("k0maru_nonexistent_test_socket_99999");
    if driver.is_available() {
        let sessions = driver
            .list_sessions()
            .expect("Should return empty list gracefully");
        assert!(sessions.is_empty(), "Dead server must yield empty Vec");
    }
}

#[test]
fn test_tmux_driver_send_prompt_session_not_found() {
    let driver = TmuxDriver::with_socket("k0maru_nonexistent_test_socket_99999");
    if driver.is_available() {
        let err = driver
            .send_prompt("%99999", "echo hello")
            .expect_err("Should fail");
        assert_eq!(err, RouterError::SessionNotFound("%99999".to_string()));
    }
}

#[test]
fn test_tmux_driver_spawn_pane_workspace_not_found_fails_fast() {
    let driver = TmuxDriver::new();
    let missing = PathBuf::from("/nonexistent/workspace/dir/xyz");
    let err = driver
        .spawn_pane(&missing, None)
        .expect_err("Missing dir must fail before tmux");
    assert_eq!(err, RouterError::WorkspaceNotFound(missing));
}

struct SocketCleaner(String);
impl Drop for SocketCleaner {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

#[test]
fn test_tmux_driver_hermetic_lifecycle_with_isolated_socket() {
    let sock = format!("k0maru_m3_hermetic_{}", std::process::id());
    let _cleaner = SocketCleaner(sock.clone());
    let driver = TmuxDriver::with_socket(&sock);
    if !driver.is_available() {
        eprintln!("tmux not available; skipping live hermetic lifecycle test");
        return;
    }

    let tmp = tempdir().unwrap();
    let canon_dir = fs::canonicalize(tmp.path()).unwrap();

    // 1. Initially empty
    let initial = driver.list_sessions().expect("List should succeed");
    assert!(initial.is_empty());

    // 2. Spawn pane in temp directory
    let spawned = driver
        .spawn_pane(tmp.path(), Some("sleep 30"))
        .expect("Spawn should succeed");
    assert!(!spawned.pane_id.is_empty());
    assert_eq!(spawned.cwd, canon_dir);

    // 3. List panes should now show the new pane
    let sessions = driver
        .list_sessions()
        .expect("List should show spawned pane");
    assert!(!sessions.is_empty());
    assert!(sessions.iter().any(|s| s.pane_id == spawned.pane_id));

    // 4. Resolve session finds the live session
    let resolution =
        resolve_session(tmp.path(), &sessions).expect("Resolve should find live session");
    assert!(resolution.is_hot());
    assert_eq!(resolution.session().unwrap().pane_id, spawned.pane_id);

    // 5. Send prompt
    driver
        .send_prompt(&spawned.pane_id, "echo test")
        .expect("Send prompt should succeed");
}
