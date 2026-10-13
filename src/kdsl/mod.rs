//! Machine-Native K-DSL 0.1 Symbolic Protocol Engine.
//!
//! Provides dense AST-anchored delta serialization, parser, and translation
//! bridges conforming to SPEC-0006.

pub mod bridge;
pub mod model;
pub mod parser;
pub mod prompt;

pub use bridge::{envelope_to_packet, estimate_tokens, packet_to_envelope, packet_token_budget};
pub use model::{AstCoordinate, AstNodeType, KdslOpcode, KdslOperation, KdslPacket};
pub use parser::parse_kdsl;
pub use prompt::{generate_kdsl_system_prompt, kdsl_contract_snippet};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_kdsl_wire_parse_and_roundtrip() {
        let wire = "!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182 §AST:F[verifyToken] @OP:SWAP_ID (Call[jwt.verify].Arg[1]) -> (Call[getPublicKey]) @OP:SET_PROP (Call[jwt.verify].Arg[2].Prop[algorithms]) -> ([\"RS256\"]) @OP:INSR_AFT (Call[jwt.verify]) -> (Await[Call[checkRevocation(decoded.jti)]]) !Σ:VERIFY(TS_COMPILE,LIVING_ADR#SEC-004)";

        let packet = KdslPacket::parse(wire).expect("Failed to parse canonical wire");
        assert_eq!(packet.version, "0.1");
        assert_eq!(packet.domain, "AUTH-SRV");
        assert_eq!(packet.filepath, "src/middleware/auth.ts");
        assert_eq!(packet.base_revision, "v4182");
        assert_eq!(packet.coordinate.node_type, AstNodeType::Function);
        assert_eq!(packet.coordinate.identifier, "verifyToken");
        assert_eq!(packet.operations.len(), 3);

        // Op 1
        assert_eq!(packet.operations[0].opcode, KdslOpcode::SwapId);
        assert_eq!(packet.operations[0].target, "Call[jwt.verify].Arg[1]");
        assert_eq!(
            packet.operations[0].replacement.as_deref(),
            Some("Call[getPublicKey]")
        );

        // Op 2
        assert_eq!(packet.operations[1].opcode, KdslOpcode::SetProp);
        assert_eq!(
            packet.operations[1].target,
            "Call[jwt.verify].Arg[2].Prop[algorithms]"
        );
        assert_eq!(
            packet.operations[1].replacement.as_deref(),
            Some("[\"RS256\"]")
        );

        // Op 3 (Nested parentheses inside replacement)
        assert_eq!(packet.operations[2].opcode, KdslOpcode::InsertAfter);
        assert_eq!(packet.operations[2].target, "Call[jwt.verify]");
        assert_eq!(
            packet.operations[2].replacement.as_deref(),
            Some("Await[Call[checkRevocation(decoded.jti)]]")
        );

        // Invariants
        assert_eq!(
            packet.verification_invariants,
            vec!["TS_COMPILE", "LIVING_ADR#SEC-004"]
        );

        // Roundtrip identity
        let serialized = packet.to_wire();
        assert_eq!(serialized, wire);
        let roundtripped = KdslPacket::parse(&serialized).unwrap();
        assert_eq!(packet, roundtripped);
    }

    #[test]
    fn test_operation_without_replacement() {
        let wire = "!δ:0.1|AUTH-SRV|src/auth.ts#rev1 §AST:C[AuthService] @OP:DEL_NODE (Method[oldHash]) !Σ:VERIFY(CARGO_CHECK)";
        let packet = KdslPacket::parse(wire).unwrap();
        assert_eq!(packet.coordinate.node_type, AstNodeType::Class);
        assert_eq!(packet.coordinate.identifier, "AuthService");
        assert_eq!(packet.operations.len(), 1);
        assert_eq!(packet.operations[0].opcode, KdslOpcode::DeleteNode);
        assert_eq!(packet.operations[0].target, "Method[oldHash]");
        assert_eq!(packet.operations[0].replacement, None);
        assert_eq!(packet.to_wire(), wire);
    }

    #[test]
    fn test_empty_invariants() {
        let wire = "!δ:0.1|CORE|src/main.rs#abc §AST:F[main] @OP:REPLACE_BODY (Block) -> (println!(\"hello\")) !Σ:VERIFY()";
        let packet = KdslPacket::parse(wire).unwrap();
        assert!(packet.verification_invariants.is_empty());
        assert_eq!(packet.to_wire(), wire);
    }

    #[test]
    fn test_custom_node_type_and_opcode() {
        let wire = "!δ:0.2|EXT|schema.graphql#head §AST:GraphQLType[User] @OP:MUTATE_FIELD (email) -> (String!) !Σ:VERIFY(GQL_LINT)";
        let packet = KdslPacket::parse(wire).unwrap();
        assert_eq!(
            packet.coordinate.node_type,
            AstNodeType::Custom("GraphQLType".to_string())
        );
        assert_eq!(
            packet.operations[0].opcode,
            KdslOpcode::Custom("MUTATE_FIELD".to_string())
        );
        assert_eq!(packet.to_wire(), wire);
    }

    #[test]
    fn test_malformed_inputs_fail_gracefully() {
        // Missing !δ:
        assert!(
            KdslPacket::parse("0.1|AUTH|src#1 §AST:F[a] @OP:DEL_NODE (x) !Σ:VERIFY()").is_err()
        );
        // Missing §AST:
        assert!(KdslPacket::parse("!δ:0.1|AUTH|src#1 F[a] @OP:DEL_NODE (x) !Σ:VERIFY()").is_err());
        // Missing !Σ:VERIFY
        assert!(KdslPacket::parse("!δ:0.1|AUTH|src#1 §AST:F[a] @OP:DEL_NODE (x)").is_err());
        // Unclosed parentheses
        assert!(
            KdslPacket::parse("!δ:0.1|AUTH|src#1 §AST:F[a] @OP:DEL_NODE (x !Σ:VERIFY()").is_err()
        );
        // No operations
        assert!(KdslPacket::parse("!δ:0.1|AUTH|src#1 §AST:F[a] !Σ:VERIFY()").is_err());
    }
}
