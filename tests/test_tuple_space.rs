//! Integration Tests for Linda Tuple Space and Associative Bus.

use assert_cmd::Command;
use k0maru_router::kdsl::{packet_to_envelope, parse_kdsl};
use k0maru_router::tuple_space::{Tuple, TuplePattern, TupleSpaceEngine};
use predicates::prelude::*;
use tempfile::tempdir;

const CANONICAL_WIRE: &str = "!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182 §AST:F[verifyToken] @OP:SWAP_ID (Call[jwt.verify].Arg[1]) -> (Call[getPublicKey]) @OP:SET_PROP (Call[jwt.verify].Arg[2].Prop[algorithms]) -> ([\"RS256\"]) @OP:INSR_AFT (Call[jwt.verify]) -> (Await[Call[checkRevocation(decoded.jti)]]) !Σ:VERIFY(TS_COMPILE,LIVING_ADR#SEC-004)";

#[test]
fn test_cli_tuple_out_rd_list_and_in_lifecycle() {
    let temp = tempdir().unwrap();
    let tuple_dir = temp.path().to_str().unwrap();

    // 1. Tuple OUT
    let mut out_cmd = Command::cargo_bin("k0maru-router").unwrap();
    out_cmd
        .args([
            "tuple",
            "out",
            "--kind",
            "TASK_READY",
            "--domain",
            "AUTH-SRV",
            "--task-id",
            "task_auth_001",
            "--payload",
            CANONICAL_WIRE,
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\": \"TASK_READY\""))
        .stdout(predicate::str::contains("\"task_id\": \"task_auth_001\""))
        .stdout(predicate::str::contains("\"domain\": \"AUTH-SRV\""));

    // 2. Tuple RD (Non-destructive)
    let mut rd_cmd = Command::cargo_bin("k0maru-router").unwrap();
    rd_cmd
        .args([
            "tuple",
            "rd",
            "--kind",
            "TASK_READY",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"task_id\": \"task_auth_001\""));

    // 3. Tuple LIST
    let mut list_cmd = Command::cargo_bin("k0maru-router").unwrap();
    list_cmd
        .args([
            "tuple",
            "list",
            "--domain",
            "AUTH-SRV",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"task_id\": \"task_auth_001\""));

    // 4. Tuple IN (Destructive claim)
    let mut in_cmd = Command::cargo_bin("k0maru-router").unwrap();
    in_cmd
        .args([
            "tuple",
            "in",
            "--kind",
            "TASK_READY",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"task_id\": \"task_auth_001\""));

    // 5. Subsequent Tuple IN returns not_found
    let mut in_again_cmd = Command::cargo_bin("k0maru-router").unwrap();
    in_again_cmd
        .args([
            "tuple",
            "in",
            "--kind",
            "TASK_READY",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"not_found\""));

    // 6. Tuple RD returns not_found
    let mut rd_empty_cmd = Command::cargo_bin("k0maru-router").unwrap();
    rd_empty_cmd
        .args([
            "tuple",
            "rd",
            "--kind",
            "TASK_READY",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"not_found\""));
}

#[test]
fn test_end_to_end_envelope_kdsl_tuple_space_closed_loop() {
    let temp = tempdir().unwrap();
    let engine = TupleSpaceEngine::new(temp.path());

    // Step 1: Master Agent prepares TaskEnvelope and converts to K-DSL wire
    let packet = parse_kdsl(CANONICAL_WIRE).unwrap();
    let envelope = packet_to_envelope(&packet);
    let wire_payload = packet.to_wire();

    // Step 2: Master publishes TASK_READY tuple into Tuple Space Bus
    let ready_tuple = Tuple::new(
        "TASK_READY",
        envelope.target.identifier.clone(),
        envelope.id.clone(),
        wire_payload.clone(),
    );
    let published = engine.out(ready_tuple).expect("Failed to out ready tuple");
    assert_eq!(published.kind, "TASK_READY");

    // Step 3: Worker Agent discovers and atomically claims task via Linda 'in'
    let claim_pattern = TuplePattern::new()
        .with_kind("TASK_READY")
        .with_domain("AUTH-SRV");
    let claimed = engine
        .take(&claim_pattern)
        .expect("Failed to claim tuple")
        .expect("No tuple available to claim");

    assert_eq!(claimed.task_id, envelope.id);
    assert_eq!(claimed.payload, wire_payload);

    // Step 4: Worker parses K-DSL wire payload and reconstructs actionable envelope
    let worker_packet = parse_kdsl(&claimed.payload).expect("Worker failed to parse K-DSL wire");
    assert_eq!(worker_packet.coordinate.identifier, "verifyToken");
    let reconstructed_envelope = packet_to_envelope(&worker_packet);
    assert_eq!(reconstructed_envelope.target.identifier, "AUTH-SRV");

    // Step 5: Worker executes task and publishes TASK_COMPLETED tuple
    let completed_tuple = Tuple::new(
        "TASK_COMPLETED",
        "AUTH-SRV",
        envelope.id.clone(),
        "{\"status\":\"passed\",\"receipt\":\"cr-eval-auth-01\"}",
    );
    engine
        .out(completed_tuple)
        .expect("Failed to publish completion tuple");

    // Step 6: Master Agent reads completion tuple non-destructively
    let completion_pattern = TuplePattern::new()
        .with_kind("TASK_COMPLETED")
        .with_task_id(&envelope.id);
    let master_check = engine
        .rd(&completion_pattern)
        .expect("Failed to read completion")
        .expect("Completion tuple not found");

    assert_eq!(master_check.task_id, envelope.id);
    assert!(master_check.payload.contains("cr-eval-auth-01"));
}

#[test]
fn test_cli_tuple_ttl_expiration() {
    let temp = tempdir().unwrap();
    let tuple_dir = temp.path().to_str().unwrap();

    // Out a tuple with 1 second TTL
    let mut out_cmd = Command::cargo_bin("k0maru-router").unwrap();
    out_cmd
        .args([
            "tuple",
            "out",
            "--kind",
            "EPHEMERAL_NOTICE",
            "--domain",
            "TEST-SRV",
            "--task-id",
            "task_temp_01",
            "--payload",
            "quick-message",
            "--ttl",
            "1",
            "--tuple-dir",
            tuple_dir,
        ])
        .assert()
        .success();

    // Verify it is readable immediately
    let mut rd_cmd = Command::cargo_bin("k0maru-router").unwrap();
    rd_cmd
        .args([
            "tuple",
            "rd",
            "--kind",
            "EPHEMERAL_NOTICE",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"task_id\": \"task_temp_01\""));

    // Sleep for 1.2s to exceed TTL
    std::thread::sleep(std::time::Duration::from_millis(1200));

    // After expiration, rd should return not_found
    let mut rd_expired = Command::cargo_bin("k0maru-router").unwrap();
    rd_expired
        .args([
            "tuple",
            "rd",
            "--kind",
            "EPHEMERAL_NOTICE",
            "--tuple-dir",
            tuple_dir,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"not_found\""));
}
