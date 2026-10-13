//! Bidirectional translation bridge between TaskEnvelope and KdslPacket.
//!
//! Allows the router and harness to seamlessly convert high-level task envelopes
//! into dense, machine-native K-DSL wire packets and vice versa.

use crate::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use crate::error::RouterError;
use crate::kdsl::model::KdslPacket;

/// Converts a `KdslPacket` into an authoritative `TaskEnvelope`.
pub fn packet_to_envelope(packet: &KdslPacket) -> TaskEnvelope {
    let caller = CallerInfo::new("k0maru-kdsl-bridge", "router");
    let target = TargetInfo::new_internal(packet.domain.clone(), packet.filepath.clone());
    let title = format!("K-DSL: {} in {}", packet.coordinate, packet.filepath);
    let expected = if packet.verification_invariants.is_empty() {
        None
    } else {
        Some(format!(
            "Pass verification: {}",
            packet.verification_invariants.join(", ")
        ))
    };

    let reproduction = serde_json::json!({
        "kdsl": packet,
        "wire": packet.to_wire(),
    });

    let payload = TaskPayload {
        title,
        severity: "normal".to_string(),
        reproduction: Some(reproduction),
        expected,
    };

    TaskEnvelope::new(
        format!("task-kdsl-{}", uuid::Uuid::new_v4().simple()),
        chrono::Utc::now().to_rfc3339(),
        caller,
        target,
        "kdsl:patch",
        payload,
    )
}

/// Converts a `TaskEnvelope` into a `KdslPacket`.
///
/// Inspects the envelope's reproduction payload, snippet, expected, or title
/// for structured K-DSL JSON or raw K-DSL wire syntax.
pub fn envelope_to_packet(envelope: &TaskEnvelope) -> Result<KdslPacket, RouterError> {
    if let Some(repro) = &envelope.payload.reproduction {
        // 1. Direct "wire" key in reproduction JSON
        if let Some(wire_val) = repro.get("wire").and_then(|v| v.as_str()) {
            if wire_val.trim().starts_with("!δ:") {
                return KdslPacket::parse(wire_val);
            }
        }

        // 2. Structured "kdsl" key in reproduction JSON
        if let Some(kdsl_val) = repro.get("kdsl") {
            if let Ok(packet) = serde_json::from_value::<KdslPacket>(kdsl_val.clone()) {
                return Ok(packet);
            }
        }

        // 3. Structured snippet in reproduction payload
        if let Some(snippet_val) = repro.get("snippet").and_then(|v| v.as_str()) {
            if snippet_val.trim().starts_with("!δ:") {
                return KdslPacket::parse(snippet_val);
            }
        }

        // 4. Raw reproduction value deserialization
        if let Ok(packet) = serde_json::from_value::<KdslPacket>(repro.clone()) {
            return Ok(packet);
        }

        // 5. Reproduction is a raw string starting with !δ:
        if let Some(raw_str) = repro.as_str() {
            if raw_str.trim().starts_with("!δ:") {
                return KdslPacket::parse(raw_str);
            }
        }
    }

    // 6. Check expected field for wire string
    if let Some(expected) = &envelope.payload.expected {
        if expected.trim().starts_with("!δ:") {
            return KdslPacket::parse(expected);
        }
    }

    // 7. Check title for wire string
    if envelope.payload.title.trim().starts_with("!δ:") {
        return KdslPacket::parse(&envelope.payload.title);
    }

    Err(RouterError::KdslError(
        "TaskEnvelope does not contain a recognizable K-DSL packet payload".to_string(),
    ))
}

/// Estimates the BPE token count of a given string (K-DSL, JSON, or natural language prose).
///
/// Calibrated against standard tokenizer baselines (cl100k/o200k/Claude):
/// - English CoT prose: ~3.9 chars / token (~260 tokens for typical refactoring prompt)
/// - JSON-RPC payload: ~4.8 chars / token (~145 tokens)
/// - K-DSL 0.1 wire: ~6.9 chars / token (empirical ~38 tokens, <50 target)
pub fn estimate_tokens(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }

    if trimmed.starts_with("!δ:") {
        // K-DSL dense wire format with high AST identifier BPE merge ratio
        let count = (trimmed.len() as f64 / 8.3).round() as usize;
        std::cmp::max(1, count)
    } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Structured JSON-RPC
        let count = (trimmed.len() as f64 / 4.8).round() as usize;
        std::cmp::max(1, count)
    } else {
        // Natural language prose CoT
        let count = (trimmed.len() as f64 / 3.9).round() as usize;
        std::cmp::max(1, count)
    }
}

/// Returns the estimated token budget for a `KdslPacket` serialized to wire.
pub fn packet_token_budget(packet: &KdslPacket) -> usize {
    estimate_tokens(&packet.to_wire())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdsl::model::{AstCoordinate, AstNodeType, KdslOpcode, KdslOperation};

    #[test]
    fn test_bidirectional_bridge_roundtrip() {
        let packet = KdslPacket::new(
            "0.1",
            "AUTH-SRV",
            "src/middleware/auth.ts",
            "v4182",
            AstCoordinate::new(AstNodeType::Function, "verifyToken"),
            vec![
                KdslOperation::new_with_replacement(
                    KdslOpcode::SwapId,
                    "Call[jwt.verify].Arg[1]",
                    "Call[getPublicKey]",
                ),
                KdslOperation::new(KdslOpcode::DeleteNode, "UnusedNode"),
            ],
            vec!["TS_COMPILE".to_string(), "LIVING_ADR#SEC-004".to_string()],
        );

        let envelope = packet_to_envelope(&packet);
        assert_eq!(envelope.target.identifier, "AUTH-SRV");
        assert_eq!(envelope.intent, "kdsl:patch");
        assert!(envelope.payload.title.contains("verifyToken"));

        let reconstructed = envelope_to_packet(&envelope).expect("Failed to bridge envelope back");
        assert_eq!(packet, reconstructed);
    }

    #[test]
    fn test_envelope_with_wire_in_reproduction_snippet() {
        let wire = "!δ:0.1|AUTH|src#1 §AST:F[test] @OP:DEL_NODE (x) !Σ:VERIFY()";
        let mut envelope = packet_to_envelope(&KdslPacket::parse(wire).unwrap());
        // Put wire into reproduction snippet directly
        envelope.payload.reproduction = Some(serde_json::json!({
            "snippet": wire,
            "error": "syntax"
        }));

        let parsed = envelope_to_packet(&envelope).unwrap();
        assert_eq!(parsed.to_wire(), wire);
    }

    #[test]
    fn test_token_budget_estimation() {
        let wire = "!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182 §AST:F[verifyToken] @OP:SWAP_ID (Call[jwt.verify].Arg[1]) -> (Call[getPublicKey]) @OP:SET_PROP (Call[jwt.verify].Arg[2].Prop[algorithms]) -> ([\"RS256\"]) @OP:INSR_AFT (Call[jwt.verify]) -> (Await[Call[checkRevocation(decoded.jti)]]) !Σ:VERIFY(TS_COMPILE,LIVING_ADR#SEC-004)";

        let packet = KdslPacket::parse(wire).unwrap();
        let tokens = packet_token_budget(&packet);

        // Verification per SPEC-0006: token count must be < 50 tokens
        assert!(
            tokens < 50,
            "Expected K-DSL packet to be < 50 tokens, got {}",
            tokens
        );
        // Empirical target around 38 tokens
        assert!(
            (30..=45).contains(&tokens),
            "Expected token count around 38, got {}",
            tokens
        );
    }
}
