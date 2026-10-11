//! Tier 5 Adversarial Coverage Hardening Suite (Part 2)
//!
//! Author: Tier 5 Challenger 2
//!
//! Rigorous white-box adversarial stress tests covering:
//! 1. Physical addressing and symlink resolution:
//!    - macOS `/tmp` vs `/private/tmp` canonical path identity and session matching.
//!    - Multiple redundant trailing slashes, relative segments (`.` and `..`), and root boundaries.
//!    - Unicode (CJK, Arabic RTL, emojis, accents) and spaces in workspace paths.
//!    - Session resolution priority matrix: Idle vs Working vs Stale deleted directories.
//! 2. Multiplexer Driver & Tmux robustness:
//!    - `classify_command_status` exhaustive matrix (shells, paths, login `-`, case sensitivity, shell-like traps).
//!    - `parse_pane_line` delimiter resilience (TSV, colon, colons inside paths, whitespace, CRLF).
//!    - Hermetic socket lifecycle & offline server fallback.
//!    - Keystroke injection defense (`-l --`).
//! 3. Router Coordinator invariants:
//!    - Dry-run zero-filesystem-mutation invariant for untrusted external tasks.
//!    - Dry-run zero-driver-mutation invariant for trusted internal tasks.
//!    - Fast failure on non-existent workspaces even under dry-run mode.
//!    - `format_agent_prompt` full permutation coverage (optional fields, structured vs raw JSON).
//!    - Coordinator outbox delegation full lifecycle and metrics.
//! 4. CLI ergonomics & error contract:
//!    - Machine-readable JSON error format on stderr across domain error types.
//!    - Unsupported driver error reporting (text and JSON).
//!    - Dry-run CLI execution (text and JSON) for internal and external tasks.
//!    - CLI status subcommand reporting with/without populated outbox.
//!    - Positional argument handling and top-level aliases lifecycle.
//!    - Clap exit code 2 on missing required arguments and unknown flags.

mod common;

use assert_cmd::Command;
use common::create_temp_dir;
use k0maru_router::addressing::{normalize_workspace_path, resolve_session, SessionResolution};
use k0maru_router::coordinator::{format_agent_prompt, RouteOutcome, RouterCoordinator};
use k0maru_router::driver::{
    classify_command_status, parse_pane_line, parse_tmux_output, LiveSession, MultiplexerDriver,
    SessionStatus, TmuxDriver,
};
use k0maru_router::envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload,
};
use k0maru_router::error::RouterError;
use k0maru_router::outbox::StagedIssueStatus;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router")
        .expect("The k0maru-router binary must be buildable via cargo_bin")
}

/// Adversarial Spy Driver recording all invocations for invariant assertions.
#[derive(Debug, Default, Clone)]
struct SpyDriver {
    pub list_sessions_calls: Arc<AtomicUsize>,
    pub send_prompt_calls: Arc<AtomicUsize>,
    pub spawn_pane_calls: Arc<AtomicUsize>,
}

impl MultiplexerDriver for SpyDriver {
    fn name(&self) -> &'static str {
        "spy-driver"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        self.list_sessions_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    fn send_prompt(&self, _pane_id: &str, _prompt: &str) -> Result<(), RouterError> {
        self.send_prompt_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        _command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        self.spawn_pane_calls.fetch_add(1, Ordering::SeqCst);
        Ok(LiveSession::new(
            "%99",
            workspace.to_path_buf(),
            SessionStatus::Idle,
            Some("bash".into()),
        ))
    }
}

fn sample_envelope_builder(id: &str, is_external: bool, workspace: Option<&str>) -> TaskEnvelope {
    TaskEnvelope {
        schema: None,
        id: id.to_string(),
        created_at: "2026-10-11T20:00:00Z".to_string(),
        caller: CallerInfo {
            workspace: "/caller/repo".to_string(),
            agent: "test-agent".to_string(),
            pane_id: Some("%1".to_string()),
        },
        target: TargetInfo {
            identifier: if is_external {
                "external-dep".to_string()
            } else {
                "internal-repo".to_string()
            },
            workspace: workspace.map(String::from),
            is_external,
        },
        intent: "bug_report".to_string(),
        payload: TaskPayload {
            title: "Adversarial test title".to_string(),
            severity: "high".to_string(),
            reproduction: Some(serde_json::json!({
                "snippet": "let x = 1;",
                "error": "syntax error"
            })),
            expected: Some("Clean compile".to_string()),
        },
    }
}

// ============================================================================
// 1. Physical Addressing and Symlink Resolution
// ============================================================================

#[test]
fn test_adversarial_t5_symlink_macos_tmp_vs_private_tmp_identity() {
    let temp = create_temp_dir();
    let temp_path = temp.path();

    // Verify canonical path of temp directory
    let canonical = normalize_workspace_path(temp_path).expect("Canonical path must succeed");

    // On macOS, std::env::temp_dir() or /tmp is typically a symlink to /private/tmp.
    // Let's create an explicit symlink inside temp directory to simulate alias behavior.
    let target_dir = temp_path.join("real_folder");
    std::fs::create_dir(&target_dir).unwrap();

    let symlink_dir = temp_path.join("symlink_alias");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target_dir, &symlink_dir).unwrap();

        let norm_target = normalize_workspace_path(&target_dir).unwrap();
        let norm_symlink = normalize_workspace_path(&symlink_dir).unwrap();

        // Both target and symlink must resolve to the identical canonical path
        assert_eq!(norm_target, norm_symlink);

        // Verify session resolution: session pointing to symlink resolves target correctly
        let session = LiveSession::new(
            "%10",
            symlink_dir.clone(),
            SessionStatus::Idle,
            Some("bash".into()),
        );
        let res1 = resolve_session(&target_dir, std::slice::from_ref(&session)).unwrap();
        assert!(res1.is_hot());
        assert_eq!(res1.session().unwrap().pane_id, "%10");

        // Inverse: session pointing to target resolves symlink target correctly
        let session_real = LiveSession::new(
            "%11",
            target_dir.clone(),
            SessionStatus::Idle,
            Some("bash".into()),
        );
        let res2 = resolve_session(&symlink_dir, &[session_real]).unwrap();
        assert!(res2.is_hot());
        assert_eq!(res2.session().unwrap().pane_id, "%11");
    }

    assert!(canonical.is_dir());
}

#[test]
fn test_adversarial_t5_path_multiple_redundant_slashes_and_dots() {
    let temp = create_temp_dir();
    let sub = temp.path().join("a").join("b");
    std::fs::create_dir_all(&sub).unwrap();

    let complex_path = format!("{}/////.///a/..//a///b///.///", temp.path().display());
    let normalized = normalize_workspace_path(&complex_path).unwrap();
    let expected = std::fs::canonicalize(&sub).unwrap();
    assert_eq!(normalized, expected);
}

#[test]
fn test_adversarial_t5_unicode_cjk_arabic_emoji_paths() {
    let temp = create_temp_dir();
    let exotic_dir = temp.path().join("ws_🚀_深度学习_مرحبا_café_résumé_∫dx_π");
    std::fs::create_dir(&exotic_dir).unwrap();

    let normalized = normalize_workspace_path(&exotic_dir).unwrap();
    assert!(normalized.is_dir());

    let session = LiveSession::new(
        "%55",
        normalized.clone(),
        SessionStatus::Idle,
        Some("zsh".into()),
    );
    let resolution = resolve_session(&exotic_dir, &[session]).unwrap();
    assert!(resolution.is_hot());
    assert_eq!(resolution.session().unwrap().pane_id, "%55");
}

#[test]
fn test_adversarial_t5_spaces_and_special_ascii_paths() {
    let temp = create_temp_dir();
    let special_dir = temp
        .path()
        .join("ws with   spaces and [special] (chars) + = @");
    std::fs::create_dir(&special_dir).unwrap();

    let normalized = normalize_workspace_path(&special_dir).unwrap();
    assert!(normalized.is_dir());

    let session = LiveSession::new(
        "%56",
        special_dir.clone(),
        SessionStatus::Working,
        Some("cargo".into()),
    );
    let resolution = resolve_session(&special_dir, &[session]).unwrap();
    assert!(resolution.is_hot());
    assert_eq!(resolution.session().unwrap().pane_id, "%56");
}

#[test]
fn test_adversarial_t5_session_resolution_multi_priority_matrix() {
    let temp = create_temp_dir();
    let target_dir = temp.path().join("target_ws");
    let other_dir = temp.path().join("other_ws");
    let deleted_dir = temp.path().join("deleted_ws");
    std::fs::create_dir(&target_dir).unwrap();
    std::fs::create_dir(&other_dir).unwrap();
    std::fs::create_dir(&deleted_dir).unwrap();
    let canon_target = std::fs::canonicalize(&target_dir).unwrap();
    let canon_other = std::fs::canonicalize(&other_dir).unwrap();

    // Now delete deleted_dir so it becomes a stale reference
    std::fs::remove_dir(&deleted_dir).unwrap();

    let session_stale =
        LiveSession::new("%0", deleted_dir, SessionStatus::Idle, Some("zsh".into()));
    let session_working_1 = LiveSession::new(
        "%1",
        canon_target.clone(),
        SessionStatus::Working,
        Some("cargo build".into()),
    );
    let session_other_idle =
        LiveSession::new("%2", canon_other, SessionStatus::Idle, Some("bash".into()));
    let session_idle = LiveSession::new(
        "%3",
        canon_target.clone(),
        SessionStatus::Idle,
        Some("zsh".into()),
    );
    let session_working_2 = LiveSession::new(
        "%4",
        canon_target.clone(),
        SessionStatus::Working,
        Some("python".into()),
    );

    // List has stale, working1, other, idle, working2.
    // Target resolution MUST select %3 (Idle in target).
    let list = vec![
        session_stale,
        session_working_1.clone(),
        session_other_idle,
        session_idle.clone(),
        session_working_2,
    ];

    let res = resolve_session(&target_dir, &list).unwrap();
    assert!(res.is_hot());
    assert_eq!(res.session().unwrap().pane_id, "%3");

    // Remove the idle session: now it must fallback to first working session (%1).
    let list_no_idle = [session_working_1, session_idle];
    let res2 = resolve_session(&target_dir, &list_no_idle[0..1]).unwrap();
    assert!(res2.is_hot());
    assert_eq!(res2.session().unwrap().pane_id, "%1");

    // Empty list: returns ColdSpawn.
    let res3 = resolve_session(&target_dir, &[]).unwrap();
    assert!(res3.is_cold());
    assert_eq!(res3.canonical_path(), &canon_target);
}

#[test]
fn test_adversarial_t5_session_resolution_predicates_and_serde() {
    let canon = PathBuf::from("/canonical/path");
    let session = LiveSession::new("%1", canon.clone(), SessionStatus::Idle, Some("sh".into()));

    let hot = SessionResolution::HotSession(session.clone());
    assert!(hot.is_hot());
    assert!(!hot.is_cold());
    assert_eq!(hot.session(), Some(&session));
    assert_eq!(hot.canonical_path(), &canon);

    let cold = SessionResolution::ColdSpawn {
        canonical_path: canon.clone(),
    };
    assert!(cold.is_cold());
    assert!(!cold.is_hot());
    assert_eq!(cold.session(), None);
    assert_eq!(cold.canonical_path(), &canon);

    // Serde roundtrip verification
    let hot_json = serde_json::to_string(&hot).unwrap();
    let hot_de: SessionResolution = serde_json::from_str(&hot_json).unwrap();
    assert_eq!(hot, hot_de);

    let cold_json = serde_json::to_string(&cold).unwrap();
    let cold_de: SessionResolution = serde_json::from_str(&cold_json).unwrap();
    assert_eq!(cold, cold_de);
}

// ============================================================================
// 2. Multiplexer Driver & Tmux Robustness
// ============================================================================

#[test]
fn test_adversarial_t5_classify_command_status_exhaustive() {
    // 1. Shells recognized as Idle
    let shells = [
        "bash", "zsh", "sh", "fish", "ksh", "csh", "tcsh", "dash", "nu",
    ];
    for s in shells {
        // Simple name
        assert_eq!(classify_command_status(s), SessionStatus::Idle);
        // Login shell prefix
        let login = format!("-{s}");
        assert_eq!(classify_command_status(&login), SessionStatus::Idle);
        // Absolute POSIX paths
        let path1 = format!("/bin/{s}");
        let path2 = format!("/usr/bin/{s}");
        let path3 = format!("/usr/local/bin/-{s}");
        assert_eq!(classify_command_status(&path1), SessionStatus::Idle);
        assert_eq!(classify_command_status(&path2), SessionStatus::Idle);
        assert_eq!(classify_command_status(&path3), SessionStatus::Idle);
        // Mixed and uppercase
        let upper = s.to_uppercase();
        assert_eq!(classify_command_status(&upper), SessionStatus::Idle);
        // Whitespace padding
        let padded = format!("  {s}  \n");
        assert_eq!(classify_command_status(&padded), SessionStatus::Idle);
    }

    // 2. Non-shells recognized as Working
    let tools = [
        "python",
        "python3",
        "node",
        "cargo",
        "cargo-watch",
        "rustc",
        "git",
        "vim",
        "nvim",
        "htop",
        "tmux",
        "ssh",
        "docker",
        "make",
        "ruby",
        "go",
    ];
    for t in tools {
        assert_eq!(classify_command_status(t), SessionStatus::Working);
        let path = format!("/usr/bin/{t}");
        assert_eq!(classify_command_status(&path), SessionStatus::Working);
    }

    // 3. Shell-like prefix/suffix traps that are NOT standard shells -> Working
    let traps = [
        "basher",
        "zshrc",
        "fish_lsp",
        "dashboard",
        "nuke",
        "csharp",
        "tcsh_runner",
    ];
    for trap in traps {
        assert_eq!(
            classify_command_status(trap),
            SessionStatus::Working,
            "Trap '{trap}' must NOT be classified as Idle"
        );
    }

    // 4. Empty, whitespace, None
    assert_eq!(classify_command_status(""), SessionStatus::Working);
    assert_eq!(classify_command_status("   "), SessionStatus::Working);
    assert_eq!(classify_command_status("\t\n"), SessionStatus::Working);
    assert_eq!(SessionStatus::from_command(None), SessionStatus::Working);
    assert_eq!(
        SessionStatus::from_command(Some("bash")),
        SessionStatus::Idle
    );
}

#[test]
fn test_adversarial_t5_parse_pane_line_delimiter_fuzzing() {
    // 1. TSV with colons in path
    let line1 = "%1\t/tmp/colon:in:path:v1\tzsh\n";
    let s1 = parse_pane_line(line1).expect("TSV with colons in path must parse");
    assert_eq!(s1.pane_id, "%1");
    assert_eq!(s1.status, SessionStatus::Idle);
    assert_eq!(s1.current_command, Some("zsh".into()));

    // 2. TSV with Windows CRLF line ending
    let line2 = "%2\t/tmp\tbash\r\n";
    let s2 = parse_pane_line(line2).expect("CRLF must be trimmed cleanly");
    assert_eq!(s2.pane_id, "%2");
    assert_eq!(s2.status, SessionStatus::Idle);

    // 3. TSV with leading whitespace before pane_id
    let line3 = "   %3\t/tmp\tsh";
    let s3 = parse_pane_line(line3).expect("Leading whitespace before %3 must be trimmed");
    assert_eq!(s3.pane_id, "%3");

    // 4. Hostile space inside pane_id must be rejected
    assert!(parse_pane_line("% 4\t/tmp\tbash").is_none());
    assert!(parse_pane_line("% 4:/tmp:bash").is_none());

    // 5. Missing % prefix must be rejected
    assert!(parse_pane_line("5\t/tmp\tbash").is_none());
    assert!(parse_pane_line("5:/tmp:bash").is_none());

    // 6. Colon-delimited line with colons in directory path
    let line6 = "%6:/tmp/repo:tag:sub:python\n";
    let s6 = parse_pane_line(line6).expect("Colon line with colons in path must parse");
    assert_eq!(s6.pane_id, "%6");
    assert_eq!(s6.status, SessionStatus::Working);
    assert_eq!(s6.current_command, Some("python".into()));

    // 7. Multi-line parse_tmux_output with corrupted and foreign garbage lines
    let stream = "\
        %10\t/tmp\tbash\n\
        \n\
        junk error: server not responding\n\
        %11\t/tmp\tcargo\n\
        0\t/tmp\tzsh\n\
        % 99\t/tmp\n\
        %12:/tmp:fish\n\
    ";
    let parsed = parse_tmux_output(stream);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].pane_id, "%10");
    assert_eq!(parsed[1].pane_id, "%11");
    assert_eq!(parsed[2].pane_id, "%12");
}

#[test]
fn test_adversarial_t5_hermetic_tmux_socket_lifecycle() {
    let driver = TmuxDriver::with_socket("k0maru_adversarial_t5_test_sock");
    assert_eq!(driver.socket(), Some("k0maru_adversarial_t5_test_sock"));

    if driver.is_available() {
        // Offline socket gracefully returns empty vec
        let sessions = driver.list_sessions().unwrap();
        assert!(sessions.is_empty());

        // send_prompt to nonexistent pane returns SessionNotFound
        let err = driver.send_prompt("%999", "hello").unwrap_err();
        match err {
            RouterError::SessionNotFound(pid) => assert_eq!(pid, "%999"),
            other => panic!("Expected SessionNotFound, got {other:?}"),
        }

        // spawn_pane with nonexistent workspace fails fast with WorkspaceNotFound
        let missing = PathBuf::from("/nonexistent/directory/12345");
        let spawn_err = driver.spawn_pane(&missing, None).unwrap_err();
        assert_eq!(spawn_err, RouterError::WorkspaceNotFound(missing));
    }
}

#[test]
fn test_adversarial_t5_keystroke_injection_defense_live() {
    let socket = "k0maru_adv_live_inject_sock";
    let driver = TmuxDriver::with_socket(socket);

    if !driver.is_available() {
        return; // Skip if tmux is not installed on host
    }

    let temp = create_temp_dir();
    let ws = temp.path();

    // Spawn a hermetic pane in temp workspace
    let spawn_res = driver.spawn_pane(ws, None);
    if let Ok(session) = spawn_res {
        let pane_id = session.pane_id;

        // Attempt injecting dangerous flags and commands:
        // 1. Strings starting with '-' that would be parsed as flags without '--'
        // 2. Command chaining like '; rm -rf /'
        // 3. Shell expansion like '$(whoami)'
        let hostile_prompts = [
            "-n -t %0",
            "--help",
            "-l",
            "; echo 'injected' > /dev/null",
            "$(whoami)",
            "C-c\nC-d",
            "Special characters: ` ~ ! @ # $ % ^ & * ( ) _ + { } [ ] | \\ : \" ; ' < > , . ? /",
        ];

        for prompt in hostile_prompts {
            let res = driver.send_prompt(&pane_id, prompt);
            assert!(
                res.is_ok(),
                "send_prompt with hostile prompt '{prompt}' must succeed via -l --"
            );
        }

        // Clean up hermetic server
        let _ = std::process::Command::new("tmux")
            .args(["-L", socket, "kill-server"])
            .output();
    }
}

// ============================================================================
// 3. Router Coordinator Invariants
// ============================================================================

#[test]
fn test_adversarial_t5_coordinator_dry_run_zero_filesystem_mutation_external() {
    let temp = create_temp_dir();
    let nonexistent_outbox = temp.path().join("completely_nonexistent_outbox_dir");

    let spy = SpyDriver::default();
    let coordinator = RouterCoordinator::new(spy.clone()).with_outbox_dir(&nonexistent_outbox);

    let env = sample_envelope_builder("task_ext_dry_1", true, None);

    // Execute dry run
    let outcome = coordinator
        .route_envelope(&env, true)
        .expect("Dry run on external task must succeed");

    assert!(outcome.is_dry_run());
    assert!(!outcome.is_staged());
    assert!(!outcome.is_dispatched());

    match outcome {
        RouteOutcome::DryRun {
            verdict,
            resolution,
        } => {
            assert!(verdict.is_untrusted());
            assert!(resolution.is_none());
        }
        _ => panic!("Expected DryRun outcome"),
    }

    // Crucial Invariant: The outbox directory was NOT created on disk!
    assert!(
        !nonexistent_outbox.exists(),
        "Dry-run mode must leave filesystem completely untouched"
    );

    // Spy driver was never called
    assert_eq!(spy.list_sessions_calls.load(Ordering::SeqCst), 0);
    assert_eq!(spy.send_prompt_calls.load(Ordering::SeqCst), 0);
    assert_eq!(spy.spawn_pane_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn test_adversarial_t5_coordinator_dry_run_zero_driver_mutation_internal() {
    let temp = create_temp_dir();
    let ws = temp.path().join("internal_ws");
    std::fs::create_dir(&ws).unwrap();

    let spy = SpyDriver::default();
    let coordinator = RouterCoordinator::new(spy.clone());

    let env = sample_envelope_builder("task_int_dry_1", false, ws.to_str());

    let outcome = coordinator
        .route_envelope(&env, true)
        .expect("Dry run on internal task must succeed");

    assert!(outcome.is_dry_run());
    match outcome {
        RouteOutcome::DryRun {
            verdict,
            resolution,
        } => {
            assert!(verdict.is_trusted());
            let res = resolution.expect("Resolution must be present for internal task");
            assert!(res.is_cold());
        }
        _ => panic!("Expected DryRun outcome"),
    }

    // Driver was queried for live sessions, but zero mutation calls were made
    assert_eq!(spy.list_sessions_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        spy.send_prompt_calls.load(Ordering::SeqCst),
        0,
        "Dry-run must not send prompt"
    );
    assert_eq!(
        spy.spawn_pane_calls.load(Ordering::SeqCst),
        0,
        "Dry-run must not spawn pane"
    );
}

#[test]
fn test_adversarial_t5_coordinator_internal_missing_workspace_fails_fast() {
    let spy = SpyDriver::default();
    let coordinator = RouterCoordinator::new(spy);

    let env = sample_envelope_builder(
        "task_int_missing_ws",
        false,
        Some("/completely/missing/dir_12345"),
    );

    // Both dry-run and live execution must fail fast with WorkspaceNotFound
    let err_dry = coordinator.route_envelope(&env, true).unwrap_err();
    assert!(matches!(err_dry, RouterError::WorkspaceNotFound(_)));

    let err_live = coordinator.route_envelope(&env, false).unwrap_err();
    assert!(matches!(err_live, RouterError::WorkspaceNotFound(_)));
}

#[test]
fn test_adversarial_t5_format_agent_prompt_all_combinations() {
    // 1. Without pane_id, without expected, without reproduction
    let mut env1 = sample_envelope_builder("task_p1", false, Some("/ws"));
    env1.caller.pane_id = None;
    env1.payload.expected = None;
    env1.payload.reproduction = None;

    let p1 = format_agent_prompt(&env1);
    assert!(p1.contains("[K0maru Task Dispatch] task_p1"));
    assert!(p1.contains("Caller: test-agent in /caller/repo\n"));
    assert!(!p1.contains("Expected Behavior:"));
    assert!(!p1.contains("Reproduction Details:"));

    // 2. Structured reproduction with only snippet
    let mut env2 = sample_envelope_builder("task_p2", false, Some("/ws"));
    env2.payload.reproduction = Some(serde_json::json!({
        "snippet": "console.log('hello')"
    }));
    let p2 = format_agent_prompt(&env2);
    assert!(p2.contains("Snippet:\n```\nconsole.log('hello')\n```"));
    assert!(!p2.contains("Error:\n```"));

    // 3. Structured reproduction with only error
    let mut env3 = sample_envelope_builder("task_p3", false, Some("/ws"));
    env3.payload.reproduction = Some(serde_json::json!({
        "error": "NullPointerException"
    }));
    let p3 = format_agent_prompt(&env3);
    assert!(p3.contains("Error:\n```\nNullPointerException\n```"));
    assert!(!p3.contains("Snippet:\n```"));

    // 4. Raw JSON reproduction (array of strings)
    let mut env4 = sample_envelope_builder("task_p4", false, Some("/ws"));
    env4.payload.reproduction = Some(serde_json::json!(["step 1", "step 2", "crash"]));
    let p4 = format_agent_prompt(&env4);
    assert!(p4.contains("```json"));
    assert!(p4.contains("\"step 1\""));

    // 5. Strongly-typed ReproductionPayload helper roundtrip
    let repro_typed = ReproductionPayload::new("const a = 1;", "TypeError: cannot reassign");
    let mut env5 = sample_envelope_builder("task_p5", false, Some("/ws"));
    env5.payload = env5.payload.with_reproduction_payload(repro_typed);
    let p5 = format_agent_prompt(&env5);
    assert!(p5.contains("const a = 1;"));
    assert!(p5.contains("TypeError: cannot reassign"));
}

#[test]
fn test_adversarial_t5_coordinator_outbox_passthrough_full_lifecycle() {
    let temp = create_temp_dir();
    let outbox = temp.path().join("outbox");
    let spy = SpyDriver::default();
    let coordinator = RouterCoordinator::new(spy).with_outbox_dir(&outbox);

    let env = sample_envelope_builder("task_coord_life_1", true, None);

    // Initial status
    let status0 = coordinator.system_status().unwrap();
    assert_eq!(status0.outbox_total(), 0);

    // Stage
    coordinator.route_envelope(&env, false).unwrap();

    let status1 = coordinator.system_status().unwrap();
    assert_eq!(status1.outbox_total(), 1);
    assert_eq!(status1.outbox_pending(), 1);
    assert_eq!(status1.outbox_approved(), 0);
    assert_eq!(status1.outbox_rejected(), 0);

    // Inspect
    let content = coordinator.inspect_staged("task_coord_life_1").unwrap();
    assert!(content.contains("task_coord_life_1"));
    assert!(content.contains("Pending Human Authorization (HITL)"));

    // Approve
    coordinator.approve_staged("task_coord_life_1").unwrap();
    let status2 = coordinator.system_status().unwrap();
    assert_eq!(status2.outbox_total(), 1);
    assert_eq!(status2.outbox_pending(), 0);
    assert_eq!(status2.outbox_approved(), 1);

    // Duplicate approve fails
    let err_dup = coordinator.approve_staged("task_coord_life_1").unwrap_err();
    assert!(matches!(err_dup, RouterError::OutboxError(_)));

    // Reject approved fails
    let err_rej = coordinator.reject_staged("task_coord_life_1").unwrap_err();
    assert!(matches!(err_rej, RouterError::OutboxError(_)));

    // List reflects Approved
    let list = coordinator.list_staged().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].status, StagedIssueStatus::Approved);
}

// ============================================================================
// 4. CLI Ergonomics & Error Contract
// ============================================================================

#[test]
fn test_adversarial_t5_cli_json_error_contract_on_failures() {
    let temp = create_temp_dir();

    // 1. Probe nonexistent directory with --json
    let probe_out = router_cmd()
        .args(["probe", "-w", "/completely/fake/workspace/98765", "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let probe_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8(probe_out).unwrap()).unwrap();
    assert_eq!(probe_err["status"], "error");
    assert_eq!(probe_err["error"], "WorkspaceNotFound");

    // 2. Route nonexistent file with --json
    let route_io_out = router_cmd()
        .args(["route", "-f", "/missing/envelope_file_123.json", "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let route_io_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8(route_io_out).unwrap()).unwrap();
    assert_eq!(route_io_err["status"], "error");
    assert_eq!(route_io_err["error"], "Io");

    // 3. Route corrupt JSON file with --json
    let corrupt_file = temp.path().join("corrupt.json");
    std::fs::write(&corrupt_file, "{ not valid json").unwrap();
    let route_json_out = router_cmd()
        .args(["route", "-f", corrupt_file.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let route_json_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8(route_json_out).unwrap()).unwrap();
    assert_eq!(route_json_err["status"], "error");
    assert_eq!(route_json_err["error"], "Json");

    // 4. Route invalid envelope (missing required fields) with --json
    let invalid_env_file = temp.path().join("invalid_env.json");
    std::fs::write(&invalid_env_file, "{\"id\": \"\"}").unwrap();
    let route_inv_out = router_cmd()
        .args(["route", "-f", invalid_env_file.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let route_inv_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8(route_inv_out).unwrap()).unwrap();
    assert_eq!(route_inv_err["status"], "error");
    // Depending on missing fields, either Json (Serde schema mismatch) or InvalidEnvelope
    assert!(route_inv_err["error"] == "Json" || route_inv_err["error"] == "InvalidEnvelope");

    // 5. Outbox approve nonexistent ID with --json
    let outbox_dir = temp.path().join("outbox");
    let approve_out = router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "nonexistent_task_id",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let approve_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8(approve_out).unwrap()).unwrap();
    assert_eq!(approve_err["status"], "error");
    assert_eq!(approve_err["error"], "IssueNotFound");

    // 6. Outbox reject nonexistent ID with --json
    let reject_out = router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            "nonexistent_task_id",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let reject_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8(reject_out).unwrap()).unwrap();
    assert_eq!(reject_err["status"], "error");
    assert_eq!(reject_err["error"], "IssueNotFound");
}

#[test]
fn test_adversarial_t5_cli_unsupported_driver_error() {
    let temp = create_temp_dir();
    let ws = temp.path();

    // 1. Plain text output
    router_cmd()
        .args(["probe", "-w", ws.to_str().unwrap(), "--driver", "zellij"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Multiplexer driver 'unsupported' unavailable",
        ))
        .stderr(predicate::str::contains("Driver 'zellij' is not supported"));

    // 2. JSON error contract output
    let json_out = router_cmd()
        .args([
            "probe",
            "-w",
            ws.to_str().unwrap(),
            "--driver",
            "zellij",
            "--json",
        ])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let err_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(json_out).unwrap()).unwrap();
    assert_eq!(err_val["status"], "error");
    assert_eq!(err_val["error"], "DriverUnavailable");
    assert!(err_val["message"].as_str().unwrap().contains("zellij"));
}

#[test]
fn test_adversarial_t5_cli_dry_run_external_and_internal() {
    let temp = create_temp_dir();
    let outbox_dir = temp.path().join("outbox");
    let ext_env = sample_envelope_builder("cli_ext_dry", true, None);
    let ext_file = temp.path().join("ext.json");
    std::fs::write(&ext_file, ext_env.to_json_pretty().unwrap()).unwrap();

    let int_ws = temp.path().join("internal_ws");
    std::fs::create_dir(&int_ws).unwrap();
    let int_env = sample_envelope_builder("cli_int_dry", false, int_ws.to_str());
    let int_file = temp.path().join("int.json");
    std::fs::write(&int_file, int_env.to_json_pretty().unwrap()).unwrap();

    // 1. External dry-run text output
    router_cmd()
        .args([
            "route",
            "-f",
            ext_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "[DRY-RUN] Task 'cli_ext_dry' routing plan:",
        ))
        .stdout(predicate::str::contains("Verdict:    UntrustedExternal"))
        .stdout(predicate::str::contains("Action:     stage to"));

    // Verify zero outbox mutation
    assert!(!outbox_dir.exists());

    // 2. External dry-run JSON output
    let ext_json_out = router_cmd()
        .args([
            "route",
            "-f",
            ext_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let ext_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(ext_json_out).unwrap()).unwrap();
    assert!(ext_val.get("DryRun").is_some());
    assert!(ext_val["DryRun"]["verdict"]["UntrustedExternal"]["reason"].is_string());
    assert!(ext_val["DryRun"]["resolution"].is_null());

    // 3. Internal dry-run text output
    router_cmd()
        .args(["route", "-f", int_file.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "[DRY-RUN] Task 'cli_int_dry' routing plan:",
        ))
        .stdout(predicate::str::contains("Verdict:    TrustedInternal"));

    // 4. Internal dry-run JSON output
    let int_json_out = router_cmd()
        .args([
            "route",
            "-f",
            int_file.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let int_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(int_json_out).unwrap()).unwrap();
    assert!(int_val.get("DryRun").is_some());
    assert_eq!(int_val["DryRun"]["verdict"], "TrustedInternal");
    assert!(int_val["DryRun"]["resolution"].is_object());
}

#[test]
fn test_adversarial_t5_cli_status_subcommand_matrix() {
    let temp = create_temp_dir();
    let outbox_dir = temp.path().join("fresh_outbox");

    // 1. Status plain text
    router_cmd()
        .args(["status", "--outbox-dir", outbox_dir.to_str().unwrap()])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("k0maru-router v"))
        .stdout(predicate::str::contains("Multiplexer Driver: tmux"))
        .stdout(predicate::str::contains("HITL Outbox"));

    // 2. Status machine-readable JSON
    let status_json_out = router_cmd()
        .args([
            "status",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let status_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(status_json_out).unwrap()).unwrap();
    assert!(status_val["version"].is_string());
    assert_eq!(status_val["driver"]["name"], "tmux");
    assert_eq!(status_val["outbox"]["total"], 0);
    assert_eq!(status_val["outbox"]["pending"], 0);
}

#[test]
fn test_adversarial_t5_cli_aliases_and_positional_matrix() {
    let temp = create_temp_dir();
    let outbox_dir = temp.path().join("outbox");
    let env_file = temp.path().join("env.json");
    let issue_id = "alias_positional_task";

    let env = sample_envelope_builder(issue_id, true, None);
    std::fs::write(&env_file, env.to_json_pretty().unwrap()).unwrap();

    // Stage
    router_cmd()
        .args([
            "route",
            env_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Positional inspect-staged
    router_cmd()
        .args([
            "inspect-staged",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(issue_id));

    // Positional approve with --json
    let approve_out = router_cmd()
        .args([
            "approve",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let app_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(approve_out).unwrap()).unwrap();
    assert_eq!(app_val["status"], "Approved");

    // Re-approve fails with exit code 1
    router_cmd()
        .args([
            "approve",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1);

    // Rejecting approved fails with exit code 1
    router_cmd()
        .args([
            "reject",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn test_adversarial_t5_cli_clap_code_2_validation() {
    // Missing required argument triggers Clap code 2
    router_cmd().arg("probe").assert().failure().code(2);
    router_cmd().arg("route").assert().failure().code(2);
    router_cmd()
        .args(["outbox", "inspect"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["outbox", "approve"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["outbox", "reject"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .arg("inspect-staged")
        .assert()
        .failure()
        .code(2);
    router_cmd().arg("approve").assert().failure().code(2);
    router_cmd().arg("reject").assert().failure().code(2);

    // Unrecognized argument triggers Clap code 2
    router_cmd()
        .args(["status", "--nonexistent-flag-99"])
        .assert()
        .failure()
        .code(2);
}
