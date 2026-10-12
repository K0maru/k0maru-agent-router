//! Comprehensive test suite for execution receipt protocol and verification engine.

use std::path::PathBuf;
use tempfile::tempdir;

use k0maru_router::error::RouterError;
use k0maru_router::receipt::{
    CompilationCheck, ExecutionChecks, ExecutionReceipt, LinterCheck, ReceiptArtifacts,
    ReceiptStore, ReceiptVerdict, ScopeGuardCheck, TestCheck, DEFAULT_RECEIPT_SCHEMA,
};

fn create_valid_receipt(task_id: &str) -> ExecutionReceipt {
    ExecutionReceipt {
        schema: DEFAULT_RECEIPT_SCHEMA.to_string(),
        task_id: task_id.to_string(),
        workspace: PathBuf::from("/workspaces/demo_project"),
        verdict: ReceiptVerdict::Success,
        checks: ExecutionChecks {
            compilation: CompilationCheck {
                status: "passed".to_string(),
                exit_code: 0,
            },
            tests: TestCheck {
                status: "passed".to_string(),
                total: 22,
                failed: 0,
                duration_ms: Some(340),
            },
            linter: LinterCheck {
                status: "passed".to_string(),
                warnings: 0,
            },
            scope_guard: ScopeGuardCheck {
                status: "passed".to_string(),
                modified_files: vec!["src/lib.rs".to_string(), "tests/lib_test.rs".to_string()],
            },
        },
        artifacts: ReceiptArtifacts {
            git_patch_sha256: Some(
                "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae".to_string(),
            ), // sha256 of "foo"
            receipt_timestamp: Some("2026-10-12T14:20:00Z".to_string()),
        },
    }
}

#[test]
fn test_receipt_spec_json_deserialization() {
    let raw_json = r#"{
        "$schema": "https://k0maru.dev/schemas/agent-receipt-v1.json",
        "task_id": "task_demo_auth_01",
        "workspace": "/path/to/.worktrees/task_demo_auth_01",
        "verdict": "success",
        "checks": {
            "compilation": { "status": "passed", "exit_code": 0 },
            "tests": { "status": "passed", "total": 22, "failed": 0, "duration_ms": 340 },
            "linter": { "status": "passed", "warnings": 0 },
            "scope_guard": { "status": "passed", "modified_files": ["src/auth.rs", "tests/auth_test.rs"] }
        },
        "artifacts": {
            "git_patch_sha256": "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
            "receipt_timestamp": "2026-10-12T14:20:00Z"
        }
    }"#;

    let receipt: ExecutionReceipt = serde_json::from_str(raw_json).expect("Must deserialize");
    assert_eq!(receipt.schema, DEFAULT_RECEIPT_SCHEMA);
    assert_eq!(receipt.task_id, "task_demo_auth_01");
    assert_eq!(receipt.verdict, ReceiptVerdict::Success);
    assert!(receipt.verdict.is_success());
    assert!(!receipt.verdict.is_failed());
    assert!(!receipt.verdict.is_blocked());
    assert!(receipt.is_passing());
    assert!(receipt.validate().is_ok());

    let summary = receipt.summary();
    assert_eq!(summary.task_id, "task_demo_auth_01");
    assert_eq!(summary.verdict, ReceiptVerdict::Success);
    assert!(summary.is_passing);
    assert!(summary.checks_summary.contains("compilation: passed"));
}

#[test]
fn test_receipt_verdict_variants_and_case_insensitivity() {
    let json_success = r#"{"verdict": "SUCCESS"}"#;
    #[derive(serde::Deserialize)]
    struct Wrapper {
        verdict: ReceiptVerdict,
    }
    let w1: Wrapper = serde_json::from_str(json_success).unwrap();
    assert_eq!(w1.verdict, ReceiptVerdict::Success);

    let json_failed = r#"{"verdict": "Failed"}"#;
    let w2: Wrapper = serde_json::from_str(json_failed).unwrap();
    assert_eq!(w2.verdict, ReceiptVerdict::Failed);
    assert!(w2.verdict.is_failed());

    let json_blocked = r#"{"verdict": "blocked"}"#;
    let w3: Wrapper = serde_json::from_str(json_blocked).unwrap();
    assert_eq!(w3.verdict, ReceiptVerdict::Blocked);
    assert!(w3.verdict.is_blocked());

    let json_invalid = r#"{"verdict": "unknown_val"}"#;
    let err = serde_json::from_str::<Wrapper>(json_invalid);
    assert!(err.is_err());
}

#[test]
fn test_receipt_sha256_verification() {
    let receipt = create_valid_receipt("task_sha_01");
    // "foo" produces SHA-256 "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae"
    let valid_patch = b"foo";
    let tampered_patch = b"foo bar";

    assert!(receipt.verify_patch_sha256(valid_patch).unwrap());
    assert!(!receipt.verify_patch_sha256(tampered_patch).unwrap());

    // If git_patch_sha256 is None, always returns true
    let mut no_sha = receipt.clone();
    no_sha.artifacts.git_patch_sha256 = None;
    assert!(no_sha.verify_patch_sha256(b"anything").unwrap());

    // If malformed sha string
    let mut bad_sha = receipt.clone();
    bad_sha.artifacts.git_patch_sha256 = Some("not_a_valid_64_char_hex".to_string());
    assert!(bad_sha.verify_patch_sha256(b"foo").is_err());
}

#[test]
fn test_receipt_validation_invariants() {
    // 1. Empty task_id
    let r = create_valid_receipt("");
    assert!(
        matches!(r.validate(), Err(RouterError::InvalidReceipt(msg)) if msg.contains("task_id"))
    );

    // 2. Path traversal in task_id
    let r_traversal = create_valid_receipt("../escaped_id");
    assert!(matches!(
        r_traversal.validate(),
        Err(RouterError::InvalidReceipt(_))
    ));
    let r_slash = create_valid_receipt("folder/id");
    assert!(matches!(
        r_slash.validate(),
        Err(RouterError::InvalidReceipt(_))
    ));

    // 3. Empty workspace
    let mut r_ws = create_valid_receipt("task_01");
    r_ws.workspace = PathBuf::from("");
    assert!(
        matches!(r_ws.validate(), Err(RouterError::InvalidReceipt(msg)) if msg.contains("workspace"))
    );

    // 4. Compilation passed but exit code != 0
    let mut r_comp = create_valid_receipt("task_02");
    r_comp.checks.compilation.exit_code = 1;
    assert!(
        matches!(r_comp.validate(), Err(RouterError::InvalidReceipt(msg)) if msg.contains("Compilation"))
    );

    // 5. Tests passed but failed > 0
    let mut r_tests = create_valid_receipt("task_03");
    r_tests.checks.tests.failed = 2;
    assert!(
        matches!(r_tests.validate(), Err(RouterError::InvalidReceipt(msg)) if msg.contains("Tests marked 'passed'"))
    );

    // 6. Test failures > total
    let mut r_test_total = create_valid_receipt("task_04");
    r_test_total.checks.tests.status = "failed".to_string();
    r_test_total.checks.tests.total = 5;
    r_test_total.checks.tests.failed = 10;
    assert!(
        matches!(r_test_total.validate(), Err(RouterError::InvalidReceipt(msg)) if msg.contains("exceeds total"))
    );

    // 7. Linter passed but warnings > 0
    let mut r_lint = create_valid_receipt("task_05");
    r_lint.checks.linter.warnings = 3;
    assert!(
        matches!(r_lint.validate(), Err(RouterError::InvalidReceipt(msg)) if msg.contains("Linter"))
    );

    // 8. Invalid timestamp
    let mut r_ts = create_valid_receipt("task_06");
    r_ts.artifacts.receipt_timestamp = Some("invalid-date-format".to_string());
    assert!(matches!(
        r_ts.validate(),
        Err(RouterError::InvalidReceipt(_))
    ));

    // 9. Invalid SHA format
    let mut r_sha = create_valid_receipt("task_07");
    r_sha.artifacts.git_patch_sha256 = Some("short_hex".to_string());
    assert!(matches!(
        r_sha.validate(),
        Err(RouterError::InvalidReceipt(_))
    ));
}

#[test]
fn test_receipt_is_passing_matrix() {
    let valid = create_valid_receipt("task_matrix");
    assert!(valid.is_passing());

    let mut failed_verdict = valid.clone();
    failed_verdict.verdict = ReceiptVerdict::Failed;
    assert!(!failed_verdict.is_passing());

    let mut blocked_verdict = valid.clone();
    blocked_verdict.verdict = ReceiptVerdict::Blocked;
    assert!(!blocked_verdict.is_passing());

    let mut failed_comp = valid.clone();
    failed_comp.checks.compilation.status = "failed".to_string();
    failed_comp.checks.compilation.exit_code = 1;
    assert!(!failed_comp.is_passing());

    let mut failed_tests = valid.clone();
    failed_tests.checks.tests.status = "failed".to_string();
    failed_tests.checks.tests.failed = 1;
    assert!(!failed_tests.is_passing());

    let mut failed_lint = valid.clone();
    failed_lint.checks.linter.status = "failed".to_string();
    failed_lint.checks.linter.warnings = 2;
    assert!(!failed_lint.is_passing());

    let mut failed_scope = valid.clone();
    failed_scope.checks.scope_guard.status = "failed".to_string();
    assert!(!failed_scope.is_passing());
}

#[test]
fn test_receipt_store_lifecycle() {
    let tmp = tempdir().unwrap();
    let store = ReceiptStore::new(tmp.path().join("receipts"));

    // List empty
    let list_empty = store.list().unwrap();
    assert!(list_empty.is_empty());

    // Get nonexistent
    assert_eq!(store.get("nonexistent_task").unwrap(), None);

    // Save valid receipt 1
    let r1 = create_valid_receipt("task_alpha");
    let path1 = store.save(&r1).unwrap();
    assert!(path1.is_file());

    // Save valid receipt 2
    let mut r2 = create_valid_receipt("task_beta");
    r2.artifacts.receipt_timestamp = Some("2026-10-12T15:00:00Z".to_string());
    store.save(&r2).unwrap();

    // Get by task_id
    let fetched1 = store.get("task_alpha").unwrap().expect("Must find r1");
    assert_eq!(fetched1.task_id, "task_alpha");
    assert_eq!(fetched1.verdict, ReceiptVerdict::Success);

    // List all (sorted by timestamp descending: task_beta then task_alpha)
    let all = store.list().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].task_id, "task_beta");
    assert_eq!(all[1].task_id, "task_alpha");

    // Path traversal rejection on get
    assert!(matches!(
        store.get("../escaped"),
        Err(RouterError::InvalidReceipt(_))
    ));

    // Delete
    let deleted = store.delete("task_alpha").unwrap();
    assert!(deleted);
    assert_eq!(store.get("task_alpha").unwrap(), None);
    let after_delete = store.list().unwrap();
    assert_eq!(after_delete.len(), 1);
    assert_eq!(after_delete[0].task_id, "task_beta");
}
