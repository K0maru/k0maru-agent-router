//! Integration and Token Compression Tests for K-DSL 0.1 Machine-Native Protocol.

use assert_cmd::Command;
use k0maru_router::kdsl::{
    envelope_to_packet, estimate_tokens, packet_to_envelope, packet_token_budget, parse_kdsl,
};
use predicates::prelude::*;
use tempfile::tempdir;

const CANONICAL_WIRE: &str = "!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182 §AST:F[verifyToken] @OP:SWAP_ID (Call[jwt.verify].Arg[1]) -> (Call[getPublicKey]) @OP:SET_PROP (Call[jwt.verify].Arg[2].Prop[algorithms]) -> ([\"RS256\"]) @OP:INSR_AFT (Call[jwt.verify]) -> (Await[Call[checkRevocation(decoded.jti)]]) !Σ:VERIFY(TS_COMPILE,LIVING_ADR#SEC-004)";

#[test]
fn test_token_compression_benchmark() {
    let wire = CANONICAL_WIRE;
    let tokens = estimate_tokens(wire);

    // SPEC-0006 non-functional target: < 50 tokens
    assert!(
        tokens < 50,
        "K-DSL wire representation exceeded target budget: {} tokens (target < 50)",
        tokens
    );

    // Parse into packet and check token budget model
    let packet = parse_kdsl(wire).expect("Failed to parse canonical wire");
    let budget = packet_token_budget(&packet);
    assert!(budget < 50);
}

#[test]
fn test_packet_to_envelope_and_back_roundtrip() {
    let packet = parse_kdsl(CANONICAL_WIRE).expect("Failed to parse canonical wire");
    let envelope = packet_to_envelope(&packet);

    assert_eq!(envelope.target.identifier, "AUTH-SRV");
    assert_eq!(envelope.intent, "kdsl:patch");
    assert!(envelope.payload.title.contains("verifyToken"));

    // Convert back from envelope to packet
    let roundtripped_packet =
        envelope_to_packet(&envelope).expect("Failed to convert envelope back to packet");
    assert_eq!(roundtripped_packet.domain, "AUTH-SRV");
    assert_eq!(roundtripped_packet.filepath, "src/middleware/auth.ts");
    assert_eq!(roundtripped_packet.coordinate.identifier, "verifyToken");
    assert_eq!(roundtripped_packet.operations.len(), 3);
    assert_eq!(
        roundtripped_packet.verification_invariants,
        vec!["TS_COMPILE", "LIVING_ADR#SEC-004"]
    );
    assert_eq!(roundtripped_packet.to_wire(), CANONICAL_WIRE);
}

#[test]
fn test_cli_kdsl_contract() {
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    cmd.args(["kdsl", "contract"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "=== K0MARU MACHINE-NATIVE PROTOCOL (K-DSL 0.1) CONTRACT ===",
        ))
        .stdout(predicate::str::contains(
            "!δ:<version>|<domain>|<filepath>#<revision>",
        ));

    let mut json_cmd = Command::cargo_bin("k0maru-router").unwrap();
    json_cmd
        .args(["kdsl", "contract", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"protocol\": \"K-DSL\""))
        .stdout(predicate::str::contains("\"version\": \"0.1\""));
}

#[test]
fn test_cli_kdsl_encode_and_decode() {
    let temp = tempdir().unwrap();
    let packet = parse_kdsl(CANONICAL_WIRE).unwrap();
    let envelope = packet_to_envelope(&packet);

    let envelope_file = temp.path().join("task_envelope.json");
    std::fs::write(
        &envelope_file,
        serde_json::to_string_pretty(&envelope).unwrap(),
    )
    .unwrap();

    // 1. CLI encode text
    let mut encode_cmd = Command::cargo_bin("k0maru-router").unwrap();
    encode_cmd
        .args(["kdsl", "encode", envelope_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182",
        ));

    // 2. CLI encode JSON
    let mut encode_json_cmd = Command::cargo_bin("k0maru-router").unwrap();
    encode_json_cmd
        .args(["kdsl", "encode", envelope_file.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""))
        .stdout(predicate::str::contains("\"wire\":"))
        .stdout(predicate::str::contains("\"estimated_tokens\":"));

    // 3. CLI decode JSON
    let mut decode_cmd = Command::cargo_bin("k0maru-router").unwrap();
    decode_cmd
        .args(["kdsl", "decode", CANONICAL_WIRE])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"domain\": \"AUTH-SRV\""))
        .stdout(predicate::str::contains("\"identifier\": \"verifyToken\""));

    // 4. CLI decode into TaskEnvelope
    let mut decode_env_cmd = Command::cargo_bin("k0maru-router").unwrap();
    decode_env_cmd
        .args(["kdsl", "decode", CANONICAL_WIRE, "--envelope"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"intent\": \"kdsl:patch\""))
        .stdout(predicate::str::contains("\"identifier\": \"AUTH-SRV\""));
}

#[test]
fn test_cli_kdsl_decode_invalid_fails() {
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    cmd.args(["kdsl", "decode", "INVALID WIRE STRING"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("K-DSL protocol error"));
}
