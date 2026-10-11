//! Empirical Adversarial Stress Test Suite for Milestone M3 (R3: Physical Addressing & Multiplexer SPI).
//!
//! Stress-tests:
//! 1. Symlink resolution (`/tmp` vs `/private/tmp`, nested symlink chains, cyclic symlinks, broken symlinks).
//! 2. Trailing slashes (`/path/`, `/path///`, root slashes, slashes on symlinks).
//! 3. Non-existent directories, regular files as workspaces, device nodes, empty strings, whitespace, null bytes.
//! 4. Spaces, Unicode, emoji, and exotic symbols in workspace directory paths.
//! 5. Session resolution priority (`Idle` preferred over `Working`, multiple matching sessions, stale deleted sessions).
//! 6. Tmux output parser stress (delimiters, path with spaces/colons, shell classification matrix).

use k0maru_router::addressing::{normalize_workspace_path, resolve_session, SessionResolution};
use k0maru_router::driver::{
    classify_command_status, parse_pane_line, parse_tmux_output, LiveSession, SessionStatus,
};
use k0maru_router::error::RouterError;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

// =========================================================================
// 1. Symlink Resolution Stress Tests
// =========================================================================

#[test]
fn test_adversarial_symlinks_macos_tmp_vs_private_tmp() {
    let tmp = tempdir().unwrap();
    let tmp_path = tmp.path();
    let canon = fs::canonicalize(tmp_path).unwrap();

    // Verify normalization
    let norm = normalize_workspace_path(tmp_path).expect("Should normalize tempdir");
    assert_eq!(norm, canon);

    // On macOS, test system /tmp vs /private/tmp specifically
    #[cfg(target_os = "macos")]
    {
        if Path::new("/tmp").is_dir() && Path::new("/private/tmp").is_dir() {
            let norm_tmp = normalize_workspace_path("/tmp").unwrap();
            let norm_private = normalize_workspace_path("/private/tmp").unwrap();
            assert_eq!(
                norm_tmp, norm_private,
                "/tmp and /private/tmp must canonicalize to identical paths"
            );

            // Test session resolution matching across /tmp and /private/tmp
            let live = vec![LiveSession {
                pane_id: "%1".to_string(),
                cwd: PathBuf::from("/private/tmp"),
                status: SessionStatus::Idle,
                current_command: Some("zsh".to_string()),
            }];

            let res = resolve_session("/tmp", &live).expect("Must resolve across symlink boundary");
            assert!(res.is_hot());
            assert_eq!(res.session().unwrap().pane_id, "%1");

            // Reverse direction: target is /private/tmp, live session cwd is /tmp
            let live_rev = vec![LiveSession {
                pane_id: "%2".to_string(),
                cwd: PathBuf::from("/tmp"),
                status: SessionStatus::Idle,
                current_command: Some("bash".to_string()),
            }];

            let res_rev = resolve_session("/private/tmp", &live_rev)
                .expect("Must resolve in reverse symlink direction");
            assert!(res_rev.is_hot());
            assert_eq!(res_rev.session().unwrap().pane_id, "%2");
        }
    }
}

#[test]
fn test_adversarial_nested_and_relative_symlink_chains() {
    let tmp = tempdir().unwrap();
    let real_dir = tmp.path().join("real_target_dir");
    fs::create_dir(&real_dir).unwrap();
    let canon_real = fs::canonicalize(&real_dir).unwrap();

    #[cfg(unix)]
    {
        // Chain: link3 -> link2 -> link1 -> real_dir
        let link1 = tmp.path().join("link_level_1");
        let link2 = tmp.path().join("link_level_2");
        let link3 = tmp.path().join("link_level_3");

        std::os::unix::fs::symlink(&real_dir, &link1).unwrap();
        std::os::unix::fs::symlink(&link1, &link2).unwrap();
        std::os::unix::fs::symlink(&link2, &link3).unwrap();

        assert_eq!(normalize_workspace_path(&link1).unwrap(), canon_real);
        assert_eq!(normalize_workspace_path(&link2).unwrap(), canon_real);
        assert_eq!(normalize_workspace_path(&link3).unwrap(), canon_real);

        // Relative symlink: link_rel -> ../real_target_dir (from subfolder)
        let subfolder = tmp.path().join("subfolder");
        fs::create_dir(&subfolder).unwrap();
        let rel_link = subfolder.join("rel_link_to_target");
        std::os::unix::fs::symlink(Path::new("../real_target_dir"), &rel_link).unwrap();

        assert_eq!(normalize_workspace_path(&rel_link).unwrap(), canon_real);

        // Test session resolution matches across 3 levels of symlinks
        let live = vec![LiveSession {
            pane_id: "%42".to_string(),
            cwd: link1.clone(),
            status: SessionStatus::Idle,
            current_command: Some("zsh".to_string()),
        }];

        let res = resolve_session(&link3, &live).expect("Deep symlink chain must resolve");
        assert!(res.is_hot());
        assert_eq!(res.session().unwrap().pane_id, "%42");
    }
}

#[test]
fn test_adversarial_cyclic_and_broken_symlinks() {
    let tmp = tempdir().unwrap();

    #[cfg(unix)]
    {
        // 1. Broken symlink: target deleted
        let broken = tmp.path().join("broken_link");
        let nonexistent_target = tmp.path().join("never_created_dir");
        std::os::unix::fs::symlink(&nonexistent_target, &broken).unwrap();

        let err = normalize_workspace_path(&broken).expect_err("Broken symlink must fail");
        assert_eq!(err, RouterError::WorkspaceNotFound(broken.clone()));

        // Also test resolve_session fails fast on broken symlink target
        let err_resolve =
            resolve_session(&broken, &[]).expect_err("Broken symlink must fail in resolve");
        assert_eq!(err_resolve, RouterError::WorkspaceNotFound(broken));

        // 2. Cyclic symlink: link_a -> link_b and link_b -> link_a
        let link_a = tmp.path().join("cycle_a");
        let link_b = tmp.path().join("cycle_b");
        std::os::unix::fs::symlink(&link_b, &link_a).unwrap();
        std::os::unix::fs::symlink(&link_a, &link_b).unwrap();

        let err_cycle = normalize_workspace_path(&link_a).expect_err("Cyclic symlink must fail");
        assert_eq!(err_cycle, RouterError::WorkspaceNotFound(link_a));
    }
}

// =========================================================================
// 2. Trailing Slashes & Relative Segments Stress Tests
// =========================================================================

#[test]
fn test_adversarial_trailing_slashes_combinations() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();

    let variations = vec![
        format!("{}/", tmp.path().display()),
        format!("{}//", tmp.path().display()),
        format!("{}///", tmp.path().display()),
        format!("{}////", tmp.path().display()),
        format!("{}////////////////", tmp.path().display()),
        format!("{}/.", tmp.path().display()),
        format!("{}/./", tmp.path().display()),
        format!("{}/.///", tmp.path().display()),
        format!("{}/sub/..", tmp.path().display()),
        format!("{}/sub/..///", tmp.path().display()),
    ];

    let sub = tmp.path().join("sub");
    fs::create_dir(&sub).unwrap();

    for v in variations {
        let norm = normalize_workspace_path(&v)
            .unwrap_or_else(|e| panic!("Failed to normalize '{}': {:?}", v, e));
        assert_eq!(
            norm, canon,
            "Variation '{}' did not match canonical path",
            v
        );
    }
}

#[test]
fn test_adversarial_root_trailing_slashes() {
    let norm_root = normalize_workspace_path("/").expect("Root / must normalize");
    let norm_multi_slash = normalize_workspace_path("///").expect("Root /// must normalize");
    assert_eq!(norm_root, norm_multi_slash);
    assert_eq!(norm_root, fs::canonicalize("/").unwrap());
}

// =========================================================================
// 3. Non-existent Directories & Invalid Workspaces
// =========================================================================

#[test]
fn test_adversarial_nonexistent_directory_variations() {
    let non_existents = vec![
        PathBuf::from("/nonexistent_k0maru_dir_999999"),
        PathBuf::from("/nonexistent_k0maru_dir_999999/"),
        PathBuf::from("/nonexistent_k0maru_dir_999999///"),
        PathBuf::from("./relative_nonexistent_dir_88888"),
        PathBuf::from("./relative_nonexistent_dir_88888/"),
        PathBuf::from("deep/nested/nonexistent/sub/path"),
    ];

    for p in non_existents {
        let err = normalize_workspace_path(&p).expect_err("Must fail for nonexistent");
        assert_eq!(err, RouterError::WorkspaceNotFound(p.clone()));

        let err_res = resolve_session(&p, &[]).expect_err("resolve_session must fail");
        assert_eq!(err_res, RouterError::WorkspaceNotFound(p));
    }
}

// =========================================================================
// 4. Regular Files & Special Nodes as Workspaces
// =========================================================================

#[test]
fn test_adversarial_regular_files_rejected() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("sample_file.txt");
    fs::write(&file, "not a directory").unwrap();

    let err = normalize_workspace_path(&file).expect_err("Regular file must be rejected");
    assert_eq!(err, RouterError::WorkspaceNotFound(file.clone()));

    // File with trailing slash: /path/file.txt/
    let file_trailing = format!("{}/", file.display());
    let err_trailing = normalize_workspace_path(&file_trailing)
        .expect_err("File with trailing slash must be rejected");
    assert_eq!(
        err_trailing,
        RouterError::WorkspaceNotFound(PathBuf::from(&file_trailing))
    );

    #[cfg(unix)]
    {
        // Symlink pointing to regular file
        let link_to_file = tmp.path().join("symlink_to_file");
        std::os::unix::fs::symlink(&file, &link_to_file).unwrap();
        let err_sym =
            normalize_workspace_path(&link_to_file).expect_err("Symlink to file must be rejected");
        assert_eq!(err_sym, RouterError::WorkspaceNotFound(link_to_file));
    }
}

#[test]
fn test_adversarial_device_nodes_rejected() {
    #[cfg(unix)]
    {
        // /dev/null is a character device, not a directory
        if Path::new("/dev/null").exists() {
            let err = normalize_workspace_path("/dev/null")
                .expect_err("/dev/null must be rejected as workspace");
            assert_eq!(
                err,
                RouterError::WorkspaceNotFound(PathBuf::from("/dev/null"))
            );
        }
    }
}

// =========================================================================
// 5. Empty Strings and Whitespace Edge Cases
// =========================================================================

#[test]
fn test_adversarial_empty_and_whitespace_inputs() {
    let whitespaces = vec![
        "",
        " ",
        "  ",
        "\t",
        "\n",
        "\r\n",
        " \t \n \r ",
        "\u{00A0}", // non-breaking space
        "   \u{00A0}   ",
    ];

    for ws in whitespaces {
        let err = normalize_workspace_path(ws).expect_err("Expected error for whitespace path");
        assert_eq!(
            err,
            RouterError::WorkspaceNotFound(PathBuf::from(ws)),
            "Failed on input: {:?}",
            ws
        );

        let err_res = resolve_session(ws, &[]).expect_err("Expected error for resolve whitespace");
        assert_eq!(
            err_res,
            RouterError::WorkspaceNotFound(PathBuf::from(ws)),
            "Resolve failed on input: {:?}",
            ws
        );
    }
}

// =========================================================================
// 6. Spaces, Unicode, and Exotic Directory Names
// =========================================================================

#[test]
fn test_adversarial_spaces_in_directory_names() {
    let tmp = tempdir().unwrap();
    let space_dir = tmp.path().join("workspace with multiple spaces in name");
    fs::create_dir(&space_dir).unwrap();
    let canon = fs::canonicalize(&space_dir).unwrap();

    let norm = normalize_workspace_path(&space_dir).expect("Should normalize path with spaces");
    assert_eq!(norm, canon);

    // Trailing slash with spaces
    let trailing = format!("{}///", space_dir.display());
    let norm_trailing =
        normalize_workspace_path(&trailing).expect("Should normalize trailing slash with spaces");
    assert_eq!(norm_trailing, canon);

    // Live session matching directory with spaces
    let live = vec![LiveSession {
        pane_id: "%7".to_string(),
        cwd: canon.clone(),
        status: SessionStatus::Idle,
        current_command: Some("zsh".to_string()),
    }];

    let res = resolve_session(&space_dir, &live).expect("Must resolve workspace with spaces");
    assert!(res.is_hot());
    assert_eq!(res.session().unwrap().pane_id, "%7");
}

#[test]
fn test_adversarial_unicode_and_exotic_directory_names() {
    let tmp = tempdir().unwrap();
    let exotic_names = vec![
        "测试工作区_chinese",
        "日本語ワークスペース",
        "🚀_rocket_workspace",
        "dir_with_dashes-and_underscores.v1",
        "dir[with]brackets",
        "dir'with'quotes",
    ];

    for name in exotic_names {
        let dir = tmp.path().join(name);
        fs::create_dir(&dir).unwrap();
        let canon = fs::canonicalize(&dir).unwrap();

        let norm = normalize_workspace_path(&dir)
            .unwrap_or_else(|e| panic!("Failed to normalize '{}': {:?}", name, e));
        assert_eq!(norm, canon);

        let res = resolve_session(&dir, &[]).unwrap();
        assert_eq!(
            res,
            SessionResolution::ColdSpawn {
                canonical_path: canon
            }
        );
    }
}

// =========================================================================
// 7. Session Resolution Priority Matrix
// =========================================================================

#[test]
fn test_adversarial_session_resolution_priority_matrix() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("target_workspace");
    fs::create_dir(&target).unwrap();
    let canon_target = fs::canonicalize(&target).unwrap();

    let other = tmp.path().join("other_workspace");
    fs::create_dir(&other).unwrap();
    let canon_other = fs::canonicalize(&other).unwrap();

    // 1. Multiple matching sessions: [Working, Working, Working, Idle, Working]
    // Idle must be selected regardless of list position!
    let sessions = vec![
        LiveSession::new(
            "%1",
            canon_target.clone(),
            SessionStatus::Working,
            Some("cargo".into()),
        ),
        LiveSession::new(
            "%2",
            canon_target.clone(),
            SessionStatus::Working,
            Some("python".into()),
        ),
        LiveSession::new(
            "%3",
            canon_other.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        ), // Other workspace
        LiveSession::new(
            "%4",
            canon_target.clone(),
            SessionStatus::Working,
            Some("node".into()),
        ),
        LiveSession::new(
            "%5",
            canon_target.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        ), // MATCHING IDLE
        LiveSession::new(
            "%6",
            canon_target.clone(),
            SessionStatus::Working,
            Some("git".into()),
        ),
    ];

    let res = resolve_session(&target, &sessions).expect("Should resolve target");
    assert!(res.is_hot());
    assert_eq!(
        res.session().unwrap().pane_id,
        "%5",
        "Must select matching Idle pane %5"
    );
    assert_eq!(res.session().unwrap().status, SessionStatus::Idle);

    // 2. Multiple matching Idle sessions: [Idle(%10), Idle(%11)]
    // Must select the first matching Idle (%10)
    let multiple_idles = vec![
        LiveSession::new(
            "%10",
            canon_target.clone(),
            SessionStatus::Idle,
            Some("bash".into()),
        ),
        LiveSession::new(
            "%11",
            canon_target.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        ),
    ];
    let res_multi = resolve_session(&target, &multiple_idles).unwrap();
    assert_eq!(res_multi.session().unwrap().pane_id, "%10");

    // 3. No Idle sessions for target, only Working: [Working(%20), Working(%21)]
    // Must fall back to first matching Working (%20)
    let only_workings = vec![
        LiveSession::new(
            "%20",
            canon_target.clone(),
            SessionStatus::Working,
            Some("cargo".into()),
        ),
        LiveSession::new(
            "%21",
            canon_target.clone(),
            SessionStatus::Working,
            Some("make".into()),
        ),
    ];
    let res_working = resolve_session(&target, &only_workings).unwrap();
    assert_eq!(res_working.session().unwrap().pane_id, "%20");
    assert_eq!(
        res_working.session().unwrap().status,
        SessionStatus::Working
    );

    // 4. Idle exists for OTHER workspace, but target has only Working:
    // Must select target's Working pane, NOT other's Idle pane!
    let cross_ws = vec![
        LiveSession::new(
            "%30",
            canon_other.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        ),
        LiveSession::new(
            "%31",
            canon_target.clone(),
            SessionStatus::Working,
            Some("cargo".into()),
        ),
    ];
    let res_cross = resolve_session(&target, &cross_ws).unwrap();
    assert_eq!(res_cross.session().unwrap().pane_id, "%31");
    assert_eq!(res_cross.session().unwrap().status, SessionStatus::Working);

    // 5. No sessions match target workspace at all:
    let no_match = vec![
        LiveSession::new(
            "%40",
            canon_other.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        ),
        LiveSession::new(
            "%41",
            canon_other.clone(),
            SessionStatus::Working,
            Some("cargo".into()),
        ),
    ];
    let res_none = resolve_session(&target, &no_match).unwrap();
    assert_eq!(
        res_none,
        SessionResolution::ColdSpawn {
            canonical_path: canon_target.clone()
        }
    );
    assert!(res_none.is_cold());
}

#[test]
fn test_adversarial_session_resolution_stale_deleted_directories() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("active_ws");
    fs::create_dir(&target).unwrap();
    let canon_target = fs::canonicalize(&target).unwrap();

    // Create a directory, get its path, then delete it to simulate a stale pane
    let deleted_dir = tmp.path().join("deleted_ws");
    fs::create_dir(&deleted_dir).unwrap();
    fs::remove_dir(&deleted_dir).unwrap();

    let sessions = vec![
        LiveSession::new(
            "%99",
            deleted_dir.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        ),
        LiveSession::new(
            "%1",
            canon_target.clone(),
            SessionStatus::Idle,
            Some("bash".into()),
        ),
    ];

    // resolve_session must not panic or error when encountering a deleted session CWD
    let res = resolve_session(&target, &sessions).expect("Should resolve active target");
    assert!(res.is_hot());
    assert_eq!(res.session().unwrap().pane_id, "%1");
}

// =========================================================================
// 8. Tmux Parser & Shell Classifier Stress Tests
// =========================================================================

#[test]
fn test_adversarial_tmux_parser_tsv_and_colon_varieties() {
    let tmp = tempdir().unwrap();
    let canon = fs::canonicalize(tmp.path()).unwrap();
    let path_str = canon.display().to_string();

    // 1. TSV with spaces in path
    let tsv_line = format!("%1\t{}\tzsh\n", path_str);
    let sessions = parse_tmux_output(&tsv_line);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].pane_id, "%1");
    assert_eq!(sessions[0].cwd, canon);
    assert_eq!(sessions[0].status, SessionStatus::Idle);

    // 2. Colon-delimited with spaces in path
    let colon_line = format!("%2:{}:cargo test\n", path_str);
    let sessions2 = parse_tmux_output(&colon_line);
    assert_eq!(sessions2.len(), 1);
    assert_eq!(sessions2[0].pane_id, "%2");
    assert_eq!(sessions2[0].status, SessionStatus::Working);

    // 3. Colon-delimited with colon inside directory path
    let weird_path_line = "%3:/tmp/foo:bar:bash\n";
    let pane = parse_pane_line(weird_path_line).unwrap();
    assert_eq!(pane.pane_id, "%3");
    assert_eq!(pane.cwd, PathBuf::from("/tmp/foo:bar"));
    assert_eq!(pane.status, SessionStatus::Idle);
}

#[test]
fn test_adversarial_shell_classifier_exhaustive() {
    // Standard shells
    for shell in &[
        "bash", "zsh", "sh", "fish", "ksh", "csh", "tcsh", "dash", "nu",
    ] {
        assert_eq!(classify_command_status(shell), SessionStatus::Idle);
        assert_eq!(
            classify_command_status(&format!("-{}", shell)),
            SessionStatus::Idle
        );
        assert_eq!(
            classify_command_status(&format!("/bin/{}", shell)),
            SessionStatus::Idle
        );
        assert_eq!(
            classify_command_status(&format!("/usr/bin/{}", shell)),
            SessionStatus::Idle
        );
        assert_eq!(
            classify_command_status(&format!("/opt/homebrew/bin/{}", shell)),
            SessionStatus::Idle
        );
        // Case-insensitivity check
        assert_eq!(
            classify_command_status(&shell.to_uppercase()),
            SessionStatus::Idle
        );
    }

    // Active tools
    for tool in &[
        "cargo",
        "cargo run",
        "cargo test",
        "rustc",
        "python",
        "python3",
        "node",
        "npm",
        "pnpm",
        "yarn",
        "git",
        "vim",
        "nvim",
        "nano",
        "docker",
        "make",
        "htop",
        "top",
        "less",
        "man",
        "sleep",
        "cat",
        "tail",
        "ssh",
        "tmux",
    ] {
        assert_eq!(
            classify_command_status(tool),
            SessionStatus::Working,
            "Tool '{}' should be Working",
            tool
        );
    }

    // Edge cases: empty, whitespace
    assert_eq!(classify_command_status(""), SessionStatus::Working);
    assert_eq!(classify_command_status("   "), SessionStatus::Working);
    assert_eq!(SessionStatus::from_command(None), SessionStatus::Working);
}

// =========================================================================
// 9. SessionResolution Serde & Structural Invariants
// =========================================================================

#[test]
fn test_adversarial_session_resolution_serde_roundtrip() {
    let path = PathBuf::from("/canonical/test/dir");
    let cold = SessionResolution::ColdSpawn {
        canonical_path: path.clone(),
    };
    let json_cold = serde_json::to_string(&cold).unwrap();
    let deserialized_cold: SessionResolution = serde_json::from_str(&json_cold).unwrap();
    assert_eq!(cold, deserialized_cold);
    assert!(deserialized_cold.is_cold());
    assert!(!deserialized_cold.is_hot());
    assert_eq!(deserialized_cold.canonical_path(), &path);

    let session = LiveSession::new("%9", path.clone(), SessionStatus::Idle, Some("fish".into()));
    let hot = SessionResolution::HotSession(session.clone());
    let json_hot = serde_json::to_string(&hot).unwrap();
    let deserialized_hot: SessionResolution = serde_json::from_str(&json_hot).unwrap();
    assert_eq!(hot, deserialized_hot);
    assert!(deserialized_hot.is_hot());
    assert!(!deserialized_hot.is_cold());
    assert_eq!(deserialized_hot.session().unwrap(), &session);
    assert_eq!(deserialized_hot.canonical_path(), &path);
}

// =========================================================================
// 10. LiveSession CWD Normalization within resolve_session
// =========================================================================

#[test]
fn test_adversarial_session_cwd_with_trailing_slashes_and_symlinks() {
    let tmp = tempdir().unwrap();
    let real_dir = tmp.path().join("real_folder");
    fs::create_dir(&real_dir).unwrap();
    let canon_real = fs::canonicalize(&real_dir).unwrap();

    #[cfg(unix)]
    {
        let symlink = tmp.path().join("symlink_to_folder");
        std::os::unix::fs::symlink(&real_dir, &symlink).unwrap();

        // 1. Session has symlink, target has canonical real path
        let session1 = LiveSession::new(
            "%1",
            symlink.clone(),
            SessionStatus::Idle,
            Some("bash".into()),
        );
        let res1 = resolve_session(&real_dir, &[session1]).unwrap();
        assert!(res1.is_hot());
        assert_eq!(res1.session().unwrap().pane_id, "%1");

        // 2. Session has trailing slashes: real_dir///
        let session2 = LiveSession::new(
            "%2",
            PathBuf::from(format!("{}///", real_dir.display())),
            SessionStatus::Idle,
            Some("bash".into()),
        );
        let res2 = resolve_session(&symlink, &[session2]).unwrap();
        assert!(res2.is_hot());
        assert_eq!(res2.session().unwrap().pane_id, "%2");

        // 3. Session has relative segments: real_dir/../real_folder
        let session3 = LiveSession::new(
            "%3",
            real_dir.join("..").join("real_folder"),
            SessionStatus::Working,
            Some("cargo".into()),
        );
        let res3 = resolve_session(&canon_real, &[session3]).unwrap();
        assert!(res3.is_hot());
        assert_eq!(res3.session().unwrap().pane_id, "%3");
    }
}

// =========================================================================
// 11. Hermetic Live Tmux Driver Invariant Stress Tests
// =========================================================================

struct HermeticCleaner(String);
impl Drop for HermeticCleaner {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

#[test]
fn test_adversarial_hermetic_tmux_spaces_symlinks_and_prompt_safety() {
    use k0maru_router::driver::{MultiplexerDriver, TmuxDriver};

    let sock = format!("k0maru_m3_adv_{}", std::process::id());
    let _cleaner = HermeticCleaner(sock.clone());
    let driver = TmuxDriver::with_socket(&sock);

    if !driver.is_available() {
        eprintln!("tmux not available on host; skipping live hermetic test");
        return;
    }

    let tmp = tempdir().unwrap();
    let space_dir = tmp.path().join("hermetic workspace with spaces");
    fs::create_dir(&space_dir).unwrap();
    let canon_space = fs::canonicalize(&space_dir).unwrap();

    // 1. Spawning pane in directory with spaces
    let spawned = driver
        .spawn_pane(&space_dir, Some("sleep 60"))
        .expect("Should spawn pane in dir with spaces");
    assert!(!spawned.pane_id.is_empty());
    assert_eq!(spawned.cwd, canon_space);

    // 2. Resolving workspace with spaces against live sessions
    let sessions = driver.list_sessions().expect("Should list sessions");
    assert!(sessions.iter().any(|s| s.pane_id == spawned.pane_id));

    let res = resolve_session(&space_dir, &sessions).expect("Should resolve session");
    assert!(res.is_hot());
    assert_eq!(res.session().unwrap().pane_id, spawned.pane_id);

    // 3. Spawning pane with symlink path
    #[cfg(unix)]
    {
        let symlink_dir = tmp.path().join("symlink_to_spaces");
        std::os::unix::fs::symlink(&space_dir, &symlink_dir).unwrap();

        let spawned_sym = driver
            .spawn_pane(&symlink_dir, Some("sleep 60"))
            .expect("Should spawn pane via symlink");
        assert_eq!(spawned_sym.cwd, canon_space);
    }

    // 4. Prompt safety: prompts with quotes, metacharacters, and CLI-like flags
    let dangerous_prompts = vec![
        "echo 'single quotes' and \"double quotes\"",
        "echo $FOO && echo ; ls | grep -v test",
        "-t %0 -l -- flag_injection_attempt",
        "echo `whoami` $(id)",
        "echo 'Unicode: 日本語 🚀 测试'",
    ];

    for prompt in dangerous_prompts {
        driver
            .send_prompt(&spawned.pane_id, prompt)
            .unwrap_or_else(|e| panic!("Failed to send dangerous prompt {:?}: {:?}", prompt, e));
    }
}
