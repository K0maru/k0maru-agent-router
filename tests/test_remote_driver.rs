//! Integration tests for RemoteSshDriver, RemoteBackend, and distributed Tailscale dispatch.

use assert_cmd::Command;
use k0maru_router::addressing::{is_local_node, resolve_remote_session};
use k0maru_router::driver::{
    LiveSession, MultiplexerDriver, RemoteBackend, RemoteSshDriver, SessionStatus,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn test_remote_backend_parsing() {
    assert_eq!(
        "auto".parse::<RemoteBackend>().unwrap(),
        RemoteBackend::Auto
    );
    assert_eq!(
        "AUTO".parse::<RemoteBackend>().unwrap(),
        RemoteBackend::Auto
    );
    assert_eq!(
        "tmux".parse::<RemoteBackend>().unwrap(),
        RemoteBackend::Tmux
    );
    assert_eq!(
        "TMUX".parse::<RemoteBackend>().unwrap(),
        RemoteBackend::Tmux
    );
    assert_eq!(
        "herdr".parse::<RemoteBackend>().unwrap(),
        RemoteBackend::Herdr
    );
    assert_eq!(
        "Herdr".parse::<RemoteBackend>().unwrap(),
        RemoteBackend::Herdr
    );

    assert!("screen".parse::<RemoteBackend>().is_err());
    assert!("zellij".parse::<RemoteBackend>().is_err());
}

#[test]
fn test_remote_ssh_driver_builder() {
    let driver = RemoteSshDriver::new("company-linux")
        .with_backend(RemoteBackend::Herdr)
        .with_ssh_bin("tailscale ssh")
        .with_timeout(15);

    assert_eq!(driver.node, "company-linux");
    assert_eq!(driver.backend, RemoteBackend::Herdr);
    assert_eq!(driver.ssh_bin, "tailscale ssh");
    assert_eq!(driver.connect_timeout_secs, 15);
    assert_eq!(driver.name(), "remote-herdr");
}

#[test]
fn test_is_local_node_matrix() {
    assert!(is_local_node(None));
    assert!(is_local_node(Some("")));
    assert!(is_local_node(Some("  ")));
    assert!(is_local_node(Some("local")));
    assert!(is_local_node(Some("LOCAL")));
    assert!(is_local_node(Some("localhost")));
    assert!(is_local_node(Some("127.0.0.1")));
    assert!(is_local_node(Some("::1")));

    assert!(!is_local_node(Some("company-linux")));
    assert!(!is_local_node(Some("ubuntu@100.88.22.33")));
    assert!(!is_local_node(Some("desktop.tailscale.net")));
}

#[test]
fn test_resolve_remote_session_logic() {
    let remote_ws = Path::new("/var/workspace/repo");
    let session_idle = LiveSession::new(
        "%1",
        PathBuf::from("/var/workspace/repo"),
        SessionStatus::Idle,
        Some("bash".into()),
    );
    let session_busy = LiveSession::new(
        "%2",
        PathBuf::from("/var/workspace/repo"),
        SessionStatus::Working,
        Some("cargo test".into()),
    );

    // Idle session preferred
    let res = resolve_remote_session(remote_ws, &[session_busy.clone(), session_idle.clone()]);
    assert_eq!(
        res,
        k0maru_router::SessionResolution::HotSession(session_idle)
    );

    // Busy session matched when no idle available
    let res2 = resolve_remote_session(remote_ws, std::slice::from_ref(&session_busy));
    assert_eq!(
        res2,
        k0maru_router::SessionResolution::HotSession(session_busy)
    );

    // Non-existent remote workspace returns ColdSpawn without failing
    let other_ws = Path::new("/opt/remote/nonexistent");
    let res3 = resolve_remote_session(other_ws, &[]);
    assert!(res3.is_cold());
    assert_eq!(res3.canonical_path(), other_ws);
}

#[test]
fn test_cli_help_includes_remote_node_flags() {
    // 1. status
    Command::cargo_bin("k0maru-router")
        .unwrap()
        .args(["status", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--node"))
        .stdout(predicates::str::contains("--remote-backend"));

    // 2. probe
    Command::cargo_bin("k0maru-router")
        .unwrap()
        .args(["probe", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--node"))
        .stdout(predicates::str::contains("--remote-backend"));

    // 3. route
    Command::cargo_bin("k0maru-router")
        .unwrap()
        .args(["route", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--node"))
        .stdout(predicates::str::contains("--remote-backend"));

    // 4. pipeline run
    Command::cargo_bin("k0maru-router")
        .unwrap()
        .args(["pipeline", "run", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--node"))
        .stdout(predicates::str::contains("--remote-backend"));
}

#[test]
fn test_mock_ssh_tmux_driver_flow() {
    let tmp = tempdir().unwrap();
    let mock_ssh = tmp.path().join("mock_ssh.sh");

    // Create a mock ssh script that handles tmux list-panes and tmux send-keys
    let script = r#"#!/bin/sh
for arg; do true; done
CMD="$arg"

if [ "$CMD" = "true" ]; then
    exit 0
elif echo "$CMD" | grep -q "command -v herdr"; then
    echo "tmux"
    exit 0
elif echo "$CMD" | grep -q "tmux list-panes"; then
    printf "%%5|/home/linux/app|zsh\n"
    exit 0
elif echo "$CMD" | grep -q "tmux load-buffer"; then
    # Stdin prompt consumption
    cat > /dev/null
    exit 0
elif echo "$CMD" | grep -q "tmux split-window"; then
    printf "%%6|/home/linux/app|zsh\n"
    exit 0
else
    echo "unknown command: $CMD" >&2
    exit 1
fi
"#;

    fs::write(&mock_ssh, script).unwrap();
    let mut perms = fs::metadata(&mock_ssh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_ssh, perms).unwrap();

    let driver = RemoteSshDriver::new("company-linux")
        .with_ssh_bin(mock_ssh.to_str().unwrap())
        .with_backend(RemoteBackend::Auto);

    assert!(driver.is_available());

    let backend = driver.detect_backend().unwrap();
    assert_eq!(backend, RemoteBackend::Tmux);

    let sessions = driver.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].pane_id, "%5");
    assert_eq!(sessions[0].cwd, PathBuf::from("/home/linux/app"));

    let send_res = driver.send_prompt("%5", "Fix the bug in src/main.rs");
    assert!(send_res.is_ok());

    let spawned = driver
        .spawn_pane(Path::new("/home/linux/app"), None)
        .unwrap();
    assert_eq!(spawned.pane_id, "%6");
}

#[test]
fn test_mock_ssh_herdr_driver_flow() {
    let tmp = tempdir().unwrap();
    let mock_ssh = tmp.path().join("mock_ssh_herdr.sh");

    // Create a mock ssh script that simulates remote Herdr CLI
    let script = r#"#!/bin/sh
for arg; do true; done
CMD="$arg"

if [ "$CMD" = "true" ]; then
    exit 0
elif echo "$CMD" | grep -q "command -v herdr"; then
    echo "herdr"
    exit 0
elif echo "$CMD" | grep -q "herdr pane list"; then
    printf '{"result":{"panes":[{"pane_id":"herdr-pane-99","cwd":"/data/herdr/repo","agent":"coder","agent_status":"idle"}]}}\n'
    exit 0
elif echo "$CMD" | grep -q "herdr agent prompt"; then
    cat > /dev/null
    exit 0
elif echo "$CMD" | grep -q "herdr pane split"; then
    printf '{"result":{"pane":{"pane_id":"herdr-pane-100","cwd":"/data/herdr/repo"}}}\n'
    exit 0
else
    echo "unknown command: $CMD" >&2
    exit 1
fi
"#;

    fs::write(&mock_ssh, script).unwrap();
    let mut perms = fs::metadata(&mock_ssh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_ssh, perms).unwrap();

    let driver = RemoteSshDriver::new("company-linux")
        .with_ssh_bin(mock_ssh.to_str().unwrap())
        .with_backend(RemoteBackend::Auto);

    assert!(driver.is_available());

    let backend = driver.detect_backend().unwrap();
    assert_eq!(backend, RemoteBackend::Herdr);

    let sessions = driver.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].pane_id, "herdr-pane-99");
    assert_eq!(sessions[0].cwd, PathBuf::from("/data/herdr/repo"));
    assert_eq!(sessions[0].status, SessionStatus::Idle);

    let prompt_res = driver.send_prompt("herdr-pane-99", "Run tests across all packages");
    assert!(prompt_res.is_ok());

    let spawned = driver
        .spawn_pane(Path::new("/data/herdr/repo"), None)
        .unwrap();
    assert_eq!(spawned.pane_id, "herdr-pane-100");
}
