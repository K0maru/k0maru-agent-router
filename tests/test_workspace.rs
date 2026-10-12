//! Comprehensive test suite for WorkspaceProvider SPI, InPlace, Cli, and Optional adapters.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

use k0maru_router::error::RouterError;
use k0maru_router::workspace::{
    CliWorkspaceProvider, InPlaceWorkspaceProvider, OptionalWorkspaceProvider, WorkspaceProvider,
};

#[tokio::test]
async fn test_inplace_workspace_provider_pass_through() {
    let provider = InPlaceWorkspaceProvider;
    assert_eq!(provider.name(), "in_place");
    assert!(provider.is_available());

    let repo = Path::new("/var/git/my-repo");
    let sandbox = provider
        .provision_sandbox("task_123", repo, Some("main"))
        .await
        .expect("Must succeed");
    assert_eq!(sandbox, repo);

    let teardown = provider.teardown_sandbox("task_123", repo).await;
    assert!(teardown.is_ok());
}

#[tokio::test]
async fn test_cli_workspace_provider_unavailable() {
    let provider = CliWorkspaceProvider::with_binary_path("/nonexistent/bin/k0maru-workspace");
    assert_eq!(provider.name(), "cli");
    assert!(!provider.is_available());

    let res = provider
        .provision_sandbox("task_123", Path::new("/repo"), None)
        .await;
    assert!(matches!(res, Err(RouterError::WorkspaceError(msg)) if msg.contains("not found")));

    let teardown = provider
        .teardown_sandbox("task_123", Path::new("/repo"))
        .await;
    assert!(matches!(teardown, Err(RouterError::WorkspaceError(msg)) if msg.contains("not found")));
}

#[tokio::test]
async fn test_cli_workspace_provider_mock_script_success_and_failure() {
    let tmp = tempdir().unwrap();
    let script_path = tmp.path().join("fake_workspace.sh");

    // Create a mock script that acts as k0maru-workspace
    let script_content = r#"#!/bin/bash
action="$1"
shift
if [ "$action" = "provision" ]; then
    task_id=""
    repo=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --task-id) task_id="$2"; shift 2 ;;
            --repo) repo="$2"; shift 2 ;;
            --base-branch) shift 2 ;;
            *) shift ;;
        esac
    done
    if [ "$task_id" = "fail_me" ]; then
        echo "Simulated provision failure" >&2
        exit 1
    fi
    echo "$repo/.worktrees/$task_id"
    exit 0
elif [ "$action" = "teardown" ]; then
    task_id=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --task-id) task_id="$2"; shift 2 ;;
            *) shift ;;
        esac
    done
    if [ "$task_id" = "fail_me" ]; then
        echo "Simulated teardown failure" >&2
        exit 1
    fi
    exit 0
fi
exit 2
"#;

    fs::write(&script_path, script_content).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let provider = CliWorkspaceProvider::with_binary_path(&script_path);
    assert!(provider.is_available());

    // Successful provision
    let repo = Path::new("/my/project");
    let sandbox = provider
        .provision_sandbox("task_demo", repo, Some("feature"))
        .await
        .unwrap();
    assert_eq!(sandbox, PathBuf::from("/my/project/.worktrees/task_demo"));

    // Successful teardown
    let teardown = provider.teardown_sandbox("task_demo", repo).await;
    assert!(teardown.is_ok());

    // Failing provision
    let fail_res = provider.provision_sandbox("fail_me", repo, None).await;
    assert!(
        matches!(fail_res, Err(RouterError::WorkspaceError(msg)) if msg.contains("Simulated provision failure"))
    );

    // Failing teardown
    let fail_td = provider.teardown_sandbox("fail_me", repo).await;
    assert!(
        matches!(fail_td, Err(RouterError::WorkspaceError(msg)) if msg.contains("Simulated teardown failure"))
    );
}

#[tokio::test]
async fn test_optional_workspace_provider_graceful_fallback() {
    // 1. Wrapping an unavailable CLI provider: should silently fall back to InPlace
    let unavailable_cli = CliWorkspaceProvider::with_binary_path("/missing/k0maru-workspace");
    let opt = OptionalWorkspaceProvider::new(unavailable_cli);

    assert_eq!(opt.name(), "cli");
    assert!(opt.is_available()); // Fallback available
    assert!(!opt.is_inner_available());

    let repo = Path::new("/workspaces/my-repo");
    let sandbox = opt
        .provision_sandbox("task_01", repo, None)
        .await
        .expect("Fallback must succeed");
    assert_eq!(sandbox, repo);

    let td = opt.teardown_sandbox("task_01", repo).await;
    assert!(td.is_ok());

    // 2. Disabled explicitly
    let opt_disabled = opt.with_enabled(false);
    assert!(!opt_disabled.is_enabled());
    assert!(!opt_disabled.is_inner_available());
    let sandbox_disabled = opt_disabled
        .provision_sandbox("task_01", repo, None)
        .await
        .unwrap();
    assert_eq!(sandbox_disabled, repo);
}

#[tokio::test]
async fn test_optional_workspace_provider_runtime_failure_fallback() {
    let tmp = tempdir().unwrap();
    let script_path = tmp.path().join("always_failing.sh");

    let script_content = r#"#!/bin/bash
echo "Fatal workspace failure" >&2
exit 1
"#;
    fs::write(&script_path, script_content).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let failing_cli = CliWorkspaceProvider::with_binary_path(&script_path);
    assert!(failing_cli.is_available());

    let opt = OptionalWorkspaceProvider::new(failing_cli);
    assert!(opt.is_inner_available());

    let repo = Path::new("/workspaces/my-repo");
    // When inner fails, OptionalWorkspaceProvider intercepts and returns repo_root from InPlace fallback
    let sandbox = opt
        .provision_sandbox("task_error", repo, None)
        .await
        .expect("Must fallback to in-place");
    assert_eq!(sandbox, repo);

    let td = opt.teardown_sandbox("task_error", repo).await;
    assert!(td.is_ok());
}
