//! Strict zero-loss parser for the K-DSL 0.1 wire grammar.
//!
//! Parses packets conforming to:
//! `!δ:<version>|<domain>|<path>#<rev> §AST:<type>[<id>] @OP:<op> (<target>) [-> (<args>)] !Σ:VERIFY(<invariants>)`

use crate::error::RouterError;
use crate::kdsl::model::{AstCoordinate, AstNodeType, KdslOpcode, KdslOperation, KdslPacket};

/// Parses a raw K-DSL 0.1 wire string into a strongly-typed `KdslPacket`.
pub fn parse_kdsl(input: &str) -> Result<KdslPacket, RouterError> {
    let trimmed = input.trim();

    // 1. Verify header prefix
    if !trimmed.starts_with("!δ:") {
        return Err(RouterError::KdslError(
            "Packet must start with '!δ:' header prefix".to_string(),
        ));
    }

    // 2. Locate coordinate prefix (§AST:)
    let ast_marker = "§AST:";
    let ast_pos = trimmed.find(ast_marker).ok_or_else(|| {
        RouterError::KdslError("Packet missing '§AST:' coordinate anchor".to_string())
    })?;

    let header_str = trimmed["!δ:".len()..ast_pos].trim();
    let (version, domain, filepath, base_revision) = parse_header(header_str)?;

    // 3. Locate verification prefix (!Σ:VERIFY()
    let verify_marker = "!Σ:VERIFY(";
    let verify_pos = trimmed.rfind(verify_marker).ok_or_else(|| {
        RouterError::KdslError("Packet missing '!Σ:VERIFY(' verification section".to_string())
    })?;

    if verify_pos < ast_pos {
        return Err(RouterError::KdslError(
            "Malformed packet: verification section appears before AST coordinate".to_string(),
        ));
    }

    // 4. Parse verification invariants
    let after_verify = &trimmed[verify_pos + verify_marker.len()..];
    let verify_end_pos = after_verify.find(')').ok_or_else(|| {
        RouterError::KdslError("Unclosed '!Σ:VERIFY(' section; expected ')'".to_string())
    })?;
    let invariants_str = after_verify[..verify_end_pos].trim();
    let verification_invariants = if invariants_str.is_empty() {
        Vec::new()
    } else {
        invariants_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    // 5. Parse AST coordinate and operations section
    // Content between §AST: and !Σ:VERIFY(
    let middle = trimmed[ast_pos + ast_marker.len()..verify_pos].trim();
    let (coordinate, ops_slice) = parse_coordinate_and_operations(middle)?;

    // 6. Parse operations
    let operations = parse_operations(ops_slice)?;
    if operations.is_empty() {
        return Err(RouterError::KdslError(
            "Packet must contain at least one @OP: operation".to_string(),
        ));
    }

    Ok(KdslPacket {
        version,
        domain,
        filepath,
        base_revision,
        coordinate,
        operations,
        verification_invariants,
    })
}

/// Parses the header slice `<version>|<domain>|<filepath>#<revision>`.
fn parse_header(s: &str) -> Result<(String, String, String, String), RouterError> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 3 {
        return Err(RouterError::KdslError(format!(
            "Invalid header format: expected '<version>|<domain>|<filepath>#<rev>', got '{}'",
            s
        )));
    }

    let version = parts[0].trim();
    let domain = parts[1].trim();
    let file_rev = parts[2].trim();

    if version.is_empty() {
        return Err(RouterError::KdslError(
            "Header version cannot be empty".to_string(),
        ));
    }
    if domain.is_empty() {
        return Err(RouterError::KdslError(
            "Header domain cannot be empty".to_string(),
        ));
    }

    let file_parts: Vec<&str> = file_rev.split('#').collect();
    if file_parts.len() != 2 {
        return Err(RouterError::KdslError(format!(
            "Invalid header file/revision format: expected '<filepath>#<revision>', got '{}'",
            file_rev
        )));
    }

    let filepath = file_parts[0].trim();
    let base_revision = file_parts[1].trim();

    if filepath.is_empty() {
        return Err(RouterError::KdslError(
            "Header filepath cannot be empty".to_string(),
        ));
    }
    if base_revision.is_empty() {
        return Err(RouterError::KdslError(
            "Header revision cannot be empty".to_string(),
        ));
    }

    Ok((
        version.to_string(),
        domain.to_string(),
        filepath.to_string(),
        base_revision.to_string(),
    ))
}

/// Parses `<type>[<identifier>] <operations...>`.
fn parse_coordinate_and_operations(s: &str) -> Result<(AstCoordinate, &str), RouterError> {
    let open_bracket = s
        .find('[')
        .ok_or_else(|| RouterError::KdslError("AST coordinate missing opening '['".to_string()))?;
    let close_bracket = s
        .find(']')
        .ok_or_else(|| RouterError::KdslError("AST coordinate missing closing ']'".to_string()))?;

    if close_bracket <= open_bracket {
        return Err(RouterError::KdslError(
            "Malformed AST coordinate brackets".to_string(),
        ));
    }

    let node_type_str = s[..open_bracket].trim();
    let identifier = s[open_bracket + 1..close_bracket].trim();

    if node_type_str.is_empty() {
        return Err(RouterError::KdslError(
            "AST node type cannot be empty".to_string(),
        ));
    }
    if identifier.is_empty() {
        return Err(RouterError::KdslError(
            "AST identifier cannot be empty".to_string(),
        ));
    }

    let coordinate = AstCoordinate {
        node_type: AstNodeType::from_wire_str(node_type_str),
        identifier: identifier.to_string(),
    };

    let remaining_ops = s[close_bracket + 1..].trim();
    Ok((coordinate, remaining_ops))
}

/// Parses a space-separated sequence of `@OP:<opcode> (<target>) [-> (<replacement>)]`.
fn parse_operations(mut s: &str) -> Result<Vec<KdslOperation>, RouterError> {
    let mut operations = Vec::new();

    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }

        if !s.starts_with("@OP:") {
            return Err(RouterError::KdslError(format!(
                "Expected operation to start with '@OP:', found '{}'",
                s
            )));
        }

        s = &s["@OP:".len()..];

        // Opcode is up to the next '(' or whitespace
        let opcode_end = s
            .find(|c: char| c.is_whitespace() || c == '(')
            .ok_or_else(|| {
                RouterError::KdslError("Operation missing target parameter".to_string())
            })?;

        let opcode_str = s[..opcode_end].trim();
        if opcode_str.is_empty() {
            return Err(RouterError::KdslError(
                "Operation opcode cannot be empty".to_string(),
            ));
        }
        let opcode = KdslOpcode::from_wire_str(opcode_str);

        s = s[opcode_end..].trim_start();

        // Target must be enclosed in balanced parentheses
        let (target, after_target) = extract_balanced_parens(s)?;
        s = after_target.trim_start();

        // Check if there is a replacement arrow '->'
        let replacement = if s.starts_with("->") {
            s = s["->".len()..].trim_start();
            if s.starts_with('(') {
                let (repl, after_repl) = extract_balanced_parens(s)?;
                s = after_repl;
                Some(repl.to_string())
            } else {
                // If not in parens, take until next whitespace or '@OP:'
                let repl_end = s.find(|c: char| c.is_whitespace()).unwrap_or(s.len());
                let repl = s[..repl_end].trim();
                s = &s[repl_end..];
                Some(repl.to_string())
            }
        } else {
            None
        };

        operations.push(KdslOperation {
            opcode,
            target: target.to_string(),
            replacement,
        });
    }

    Ok(operations)
}

/// Extracts content enclosed by the outermost balanced '(' and ')' parentheses.
/// Returns `(inner_content, remaining_slice)`.
fn extract_balanced_parens(input: &str) -> Result<(&str, &str), RouterError> {
    if !input.starts_with('(') {
        return Err(RouterError::KdslError(format!(
            "Expected '(' at start of parameter, found: '{}'",
            input.chars().take(20).collect::<String>()
        )));
    }

    let mut depth = 0;
    let mut closing_idx = None;

    for (idx, ch) in input.char_indices() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                closing_idx = Some(idx);
                break;
            }
        }
    }

    match closing_idx {
        Some(idx) => Ok((&input[1..idx], &input[idx + 1..])),
        None => Err(RouterError::KdslError(
            "Unclosed parenthesis in operation parameter".to_string(),
        )),
    }
}
