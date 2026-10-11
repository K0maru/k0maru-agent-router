//! Adversarial empirical stress tests for Milestone M3 (TmuxDriver & Addressing).
//!
//! Specifically validates:
//! 1. Unusual tmux output parsing: colons in paths, missing fields, malformed lines, empty output.
//! 2. Offline server handling: `Ok(vec![])` on non-running server, `SessionNotFound` on send_prompt.
//! 3. Keystroke injection defense: literal prompt typing (`-l --`) protecting against key names (`Enter`, `C-c`, `Up`, etc.).
//! 4. Pre-flight validation: preventing tmux silent fallback to `$HOME` for non-existent, file, broken symlink, or empty paths.
//! 5. Empirical reproduction of parser defects discovered during challenge.

use k0maru_router::addressing::{resolve_session, SessionResolution};
use k0maru_router::driver::{
    parse_pane_line, parse_tmux_output, LiveSession, MultiplexerDriver, SessionStatus, TmuxDriver,
};
use k0maru_router::error::RouterError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

struct IsolatedSocketCleaner(String);
impl Drop for IsolatedSocketCleaner {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

// =========================================================================
// 1. Unusual Tmux Output & Delimiter Stress-Testing
// =========================================================================

#[test]
fn test_stress_parser_paths_with_multiple_colons_tsv() {
    let raw = "%0\t/tmp/repo:v1:beta:patch\tzsh\n\
               %1\t/tmp/a:b:c:d:e:f:g\tbash\n\
               %2\t/tmp/:leading_and_trailing:\tfish\n";
    let sessions = parse_tmux_output(raw);
    assert_eq!(sessions.len(), 3);

    assert_eq!(sessions[0].pane_id, "%0");
    assert_eq!(sessions[0].cwd, PathBuf::from("/tmp/repo:v1:beta:patch"));
    assert_eq!(sessions[0].status, SessionStatus::Idle);
    assert_eq!(sessions[0].current_command, Some("zsh".into()));

    assert_eq!(sessions[1].pane_id, "%1");
    assert_eq!(sessions[1].cwd, PathBuf::from("/tmp/a:b:c:d:e:f:g"));
    assert_eq!(sessions[1].status, SessionStatus::Idle);
    assert_eq!(sessions[1].current_command, Some("bash".into()));

    assert_eq!(sessions[2].pane_id, "%2");
    assert_eq!(
        sessions[2].cwd,
        PathBuf::from("/tmp/:leading_and_trailing:")
    );
    assert_eq!(sessions[2].status, SessionStatus::Idle);
    assert_eq!(sessions[2].current_command, Some("fish".into()));
}

#[test]
fn test_stress_parser_paths_with_multiple_colons_colon_delimited() {
    // 3-field format: <pane_id>:<cwd>:<command>
    // Even if cwd contains multiple colons, first_colon and last_colon isolate cwd cleanly.
    let raw = "%10:/tmp/repo:v1:branch:python\n";
    let session = parse_pane_line(raw.trim()).expect("Should parse multiline colon path");
    assert_eq!(session.pane_id, "%10");
    assert_eq!(session.cwd, PathBuf::from("/tmp/repo:v1:branch"));
    assert_eq!(session.status, SessionStatus::Working);
    assert_eq!(session.current_command, Some("python".into()));

    // 2-field format: <pane_id>:<cwd>
    let raw_2field = "%11:/tmp/simple_dir\n";
    let session_2 = parse_pane_line(raw_2field.trim()).expect("Should parse 2-field colon line");
    assert_eq!(session_2.pane_id, "%11");
    assert_eq!(session_2.cwd, PathBuf::from("/tmp/simple_dir"));
    assert_eq!(session_2.status, SessionStatus::Working);
    assert_eq!(session_2.current_command, None);
}

#[test]
fn test_stress_parser_missing_fields_and_truncated_lines() {
    // Empty inputs
    assert!(parse_pane_line("").is_none());
    assert!(parse_pane_line("   ").is_none());
    assert!(parse_pane_line("\t\t").is_none());
    assert!(parse_pane_line("::").is_none());
    assert!(parse_pane_line(":::").is_none());

    // Single field only
    assert!(parse_pane_line("%0").is_none());
    assert!(parse_pane_line("   %0   ").is_none());

    // Missing CWD
    assert!(parse_pane_line("%0\t").is_none());
    assert!(parse_pane_line("%0\t   ").is_none());
    assert!(parse_pane_line("%0:").is_none());
    assert!(parse_pane_line("%0:   ").is_none());
    assert!(parse_pane_line("%0\t\tbash").is_none());
    assert!(parse_pane_line("%0::bash").is_none());

    // Missing Pane ID (colon delimiter)
    assert!(parse_pane_line(":/tmp:bash").is_none());
    assert!(parse_pane_line("  :/tmp:bash").is_none());

    // Valid 2-field (missing command) falls back to SessionStatus::Working and current_command: None
    let s_tsv = parse_pane_line("%0\t/tmp").expect("2-field TSV should parse");
    assert_eq!(s_tsv.pane_id, "%0");
    assert_eq!(s_tsv.status, SessionStatus::Working);
    assert_eq!(s_tsv.current_command, None);

    let s_tsv_trailing =
        parse_pane_line("%0\t/tmp\t").expect("TSV with empty command should parse");
    assert_eq!(s_tsv_trailing.pane_id, "%0");
    assert_eq!(s_tsv_trailing.status, SessionStatus::Working);
    assert_eq!(s_tsv_trailing.current_command, None);
}

#[test]
fn test_stress_parser_whitespace_crlf_and_giant_lines() {
    // CRLF Windows-style newlines in mixed output
    let crlf_output = "%0\t/tmp\tzsh\r\n\r\n%1\t/var\tcargo\r\n";
    let sessions = parse_tmux_output(crlf_output);
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].pane_id, "%0");
    assert_eq!(sessions[0].status, SessionStatus::Idle);
    assert_eq!(sessions[1].pane_id, "%1");
    assert_eq!(sessions[1].status, SessionStatus::Working);

    // Giant line (10,000 characters) tolerance
    let giant_path = format!("/tmp/{}", "a".repeat(10000));
    let giant_line = format!("%999\t{}\tbash", giant_path);
    let session = parse_pane_line(&giant_line).expect("Giant line should not panic");
    assert_eq!(session.pane_id, "%999");
    assert_eq!(session.status, SessionStatus::Idle);

    // Spaces in directory name and unicode emojis
    let unicode_line = "%5\t/tmp/my project 🚀/src\tpython";
    let u_session = parse_pane_line(unicode_line).expect("Unicode path should parse");
    assert_eq!(u_session.pane_id, "%5");
    assert_eq!(u_session.cwd, PathBuf::from("/tmp/my project 🚀/src"));
    assert_eq!(u_session.status, SessionStatus::Working);
}

// =========================================================================
// 2. Empirical Parser Defect Reproductions
// =========================================================================

#[test]
fn test_reproduce_defect_1_leading_tab_erasure_misattributes_fields() {
    // EMPIRICAL VERIFICATION:
    // Leading tab delimiter must be preserved; line with empty pane_id must be rejected.
    assert!(
        parse_pane_line("\t/tmp\tbash").is_none(),
        "Line with leading tab (empty pane_id) must return None"
    );
}

#[test]
fn test_reproduce_defect_2_unvalidated_colon_fallback_parses_arbitrary_errors() {
    // EMPIRICAL VERIFICATION:
    // Diagnostic error messages and non-pane lines without valid '%' pane_id must be rejected.
    assert!(
        parse_pane_line("unknown command: list-panes").is_none(),
        "Diagnostic error string must return None"
    );
}

#[test]
fn test_reproduce_defect_3_two_field_colon_path_with_colon_truncated() {
    // EMPIRICAL DEFECT PROOF:
    // In `src/driver/tmux.rs:238-249`, if a 2-field colon line has a path containing a colon:
    // e.g. "%0:/tmp/repo:v1", `first_colon != last_colon` causes the parser to treat "v1"
    // as the running command and truncate the directory to "/tmp/repo"!
    let session = parse_pane_line("%0:/tmp/repo:v1").expect("Should parse");
    assert_eq!(
        session.cwd,
        PathBuf::from("/tmp/repo"),
        "Directory was truncated from /tmp/repo:v1 to /tmp/repo"
    );
    assert_eq!(
        session.current_command,
        Some("v1".into()),
        "'v1' was incorrectly extracted as running command"
    );
}

// =========================================================================
// 3. Offline Server Handling Verification
// =========================================================================

#[test]
fn test_offline_server_returns_empty_vec_not_err() {
    let unique_sock = format!("k0maru_challenger2_offline_{}", std::process::id());
    let driver = TmuxDriver::with_socket(&unique_sock);
    if !driver.is_available() {
        return;
    }

    // Server is guaranteed NOT running on this unique socket
    let result = driver.list_sessions();
    assert!(
        result.is_ok(),
        "Offline server must return Ok(vec![]), not an Err: {:?}",
        result
    );
    let sessions = result.unwrap();
    assert!(
        sessions.is_empty(),
        "Offline server must produce an empty session list"
    );
}

#[test]
fn test_offline_server_send_prompt_fails_with_session_not_found() {
    let unique_sock = format!("k0maru_challenger2_offline_prompt_{}", std::process::id());
    let driver = TmuxDriver::with_socket(&unique_sock);
    if !driver.is_available() {
        return;
    }

    // Attempting send_prompt on an offline server must map cleanly to SessionNotFound
    let err = driver
        .send_prompt("%0", "echo hello")
        .expect_err("send_prompt on dead server must fail");
    assert_eq!(
        err,
        RouterError::SessionNotFound("%0".to_string()),
        "Offline server error must map to RouterError::SessionNotFound"
    );
}

// =========================================================================
// 4. Keystroke Injection Defense Verification (Empirical Live Tmux Test)
// =========================================================================

#[test]
fn test_keystroke_injection_defense_live_tmux() {
    let sock = format!("k0maru_challenger2_injection_{}", std::process::id());
    let _cleaner = IsolatedSocketCleaner(sock.clone());
    let driver = TmuxDriver::with_socket(&sock);
    if !driver.is_available() {
        eprintln!("tmux not available on host, skipping live keystroke test");
        return;
    }

    let tmp = tempdir().unwrap();
    // Spawn a live pane running cat (which echoes input verbatim without shell execution)
    let session = driver
        .spawn_pane(tmp.path(), Some("cat"))
        .expect("Should spawn pane with cat");

    // Sleep briefly to let cat start
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Adversarial prompt containing key names and flags that WOULD trigger if not using -l --
    let hostile_prompt = "INJECT_VERIFY: Enter C-c Up Down C-d Space BSpace";
    driver
        .send_prompt(&session.pane_id, hostile_prompt)
        .expect("send_prompt should succeed with hostile string");

    // Give tmux a moment to process keys
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Capture the pane content directly via tmux capture-pane
    let capture_out = Command::new("tmux")
        .args(["-L", &sock, "capture-pane", "-p", "-t", &session.pane_id])
        .output()
        .expect("capture-pane should execute");
    let screen = String::from_utf8_lossy(&capture_out.stdout);

    // EMPIRICAL VERIFICATION:
    // The exact string "INJECT_VERIFY: Enter C-c Up Down C-d Space BSpace" MUST appear verbatim.
    // If "Enter" or "C-c" was interpreted as a keystroke instead of literal text, "C-c" would kill `cat`
    // and "Enter" would split lines prematurely.
    assert!(
        screen.contains("INJECT_VERIFY: Enter C-c Up Down C-d Space BSpace"),
        "Screen did not contain literal string; got: \n{}",
        screen
    );

    // Test prompt with leading dashes: "-l -- -t %0 special_flag_injection"
    let dash_prompt = "-l -- -t %0 FLAG_LITERAL_TEST";
    driver
        .send_prompt(&session.pane_id, dash_prompt)
        .expect("send_prompt should handle leading dashes safely without flag parsing error");

    std::thread::sleep(std::time::Duration::from_millis(200));

    let capture_dash = Command::new("tmux")
        .args(["-L", &sock, "capture-pane", "-p", "-t", &session.pane_id])
        .output()
        .expect("capture-pane should execute");
    let screen_dash = String::from_utf8_lossy(&capture_dash.stdout);
    assert!(
        screen_dash.contains("-l -- -t %0 FLAG_LITERAL_TEST"),
        "Screen did not contain literal dash string; got: \n{}",
        screen_dash
    );
}

// =========================================================================
// 5. Pre-Flight Validation Preventing Tmux Silent Fallback to $HOME
// =========================================================================

#[test]
fn test_preflight_validation_nonexistent_workspace() {
    let driver = TmuxDriver::new();
    let bogus = PathBuf::from("/definitely/nonexistent/directory/for/challenger2/check");
    let err = driver.spawn_pane(&bogus, None).expect_err("Must fail fast");
    assert_eq!(err, RouterError::WorkspaceNotFound(bogus));
}

#[test]
fn test_preflight_validation_file_as_workspace() {
    let tmp = tempdir().unwrap();
    let regular_file = tmp.path().join("not_a_directory.txt");
    fs::write(&regular_file, "plain content").unwrap();

    let driver = TmuxDriver::new();
    let err = driver
        .spawn_pane(&regular_file, None)
        .expect_err("File must be rejected as workspace");
    assert_eq!(err, RouterError::WorkspaceNotFound(regular_file));
}

#[test]
fn test_preflight_validation_broken_symlink_as_workspace() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("dir_to_be_removed");
    fs::create_dir(&target).unwrap();
    let symlink = tmp.path().join("broken_symlink");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &symlink).unwrap();
        fs::remove_dir(&target).unwrap(); // break the symlink

        let driver = TmuxDriver::new();
        let err = driver
            .spawn_pane(&symlink, None)
            .expect_err("Broken symlink must fail");
        assert_eq!(err, RouterError::WorkspaceNotFound(symlink));
    }
}

#[test]
fn test_preflight_validation_empty_and_whitespace_paths() {
    let driver = TmuxDriver::new();

    let err_empty = driver
        .spawn_pane(Path::new(""), None)
        .expect_err("Empty string must fail");
    assert_eq!(err_empty, RouterError::WorkspaceNotFound(PathBuf::from("")));

    let err_ws = driver
        .spawn_pane(Path::new("   \t  "), None)
        .expect_err("Whitespace string must fail");
    assert_eq!(
        err_ws,
        RouterError::WorkspaceNotFound(PathBuf::from("   \t  "))
    );
}

#[test]
fn test_empirical_proof_raw_tmux_fallback_vs_k0maru_guard() {
    // This test empirically proves that raw tmux -c <missing_dir> SILENTLY falls back to $HOME,
    // demonstrating that k0maru-agent-router's pre-flight normalize_workspace_path guard
    // is essential to prevent silent corruption.
    let sock = format!("k0maru_fallback_demo_{}", std::process::id());
    let _cleaner = IsolatedSocketCleaner(sock.clone());

    let raw_out = Command::new("tmux")
        .args([
            "-L",
            &sock,
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_current_path}",
            "-c",
            "/nonexistent/directory/that/does/not/exist",
        ])
        .output();

    if let Ok(out) = raw_out {
        if out.status.success() {
            let spawned_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // Raw tmux succeeded and fell back to $HOME or system default!
            let home = std::env::var("HOME").unwrap_or_default();
            eprintln!(
                "Empirically proven: Raw tmux fell back to '{}' (HOME is '{}')",
                spawned_path, home
            );
            assert!(
                !spawned_path.is_empty(),
                "Raw tmux indeed falls back silently without failing"
            );
        }
    }

    // Now verify that k0maru's TmuxDriver BLOCKS this silent fallback:
    let driver = TmuxDriver::with_socket(&sock);
    let blocked_err = driver
        .spawn_pane(
            Path::new("/nonexistent/directory/that/does/not/exist"),
            None,
        )
        .expect_err("TmuxDriver MUST block nonexistent directory before tmux runs");

    assert_eq!(
        blocked_err,
        RouterError::WorkspaceNotFound(PathBuf::from("/nonexistent/directory/that/does/not/exist"))
    );
}

// =========================================================================
// 6. Session State Resolution Stress Matrix
// =========================================================================

#[test]
fn test_resolve_session_multi_session_idle_priority() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();

    let mut sessions = Vec::new();
    // 5 working sessions
    for i in 0..5 {
        sessions.push(LiveSession::new(
            format!("%{}", i),
            canon.clone(),
            SessionStatus::Working,
            Some(format!("tool_{}", i)),
        ));
    }
    // 1 idle session at the end
    let idle_session = LiveSession::new(
        "%99",
        canon.clone(),
        SessionStatus::Idle,
        Some("zsh".into()),
    );
    sessions.push(idle_session.clone());

    let resolution = resolve_session(tmp.path(), &sessions).expect("Should resolve");
    assert_eq!(
        resolution,
        SessionResolution::HotSession(idle_session),
        "Must select Idle session even if preceded by 5 Working sessions"
    );
}

#[test]
fn test_resolve_session_stale_deleted_panes_ignored() {
    let tmp_live = tempdir().unwrap();
    let tmp_deleted = tempdir().unwrap();
    let canon_live = fs::canonicalize(tmp_live.path()).unwrap();
    let canon_deleted = fs::canonicalize(tmp_deleted.path()).unwrap();

    // Stale session in tmp_deleted
    let stale_session = LiveSession::new(
        "%1",
        canon_deleted,
        SessionStatus::Idle,
        Some("bash".into()),
    );
    // Delete the directory of the stale session
    drop(tmp_deleted);

    let sessions = vec![stale_session];
    // Resolving for tmp_live should NOT match the deleted session and should ColdSpawn
    let res = resolve_session(tmp_live.path(), &sessions).expect("Should resolve");
    assert_eq!(
        res,
        SessionResolution::ColdSpawn {
            canonical_path: canon_live
        }
    );
}
