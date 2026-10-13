//! K-DSL 0.1 AST-anchored delta packet data model and types.
//!
//! Provides the strongly-typed AST coordinate, RFC 6902-inspired discrete delta operations,
//! and verification invariant data structures conforming to SPEC-0006.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::RouterError;

/// High-level K-DSL 0.1 AST delta packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdslPacket {
    /// Wire protocol version, e.g. "0.1".
    pub version: String,
    /// Destination domain or subsystem, e.g. "AUTH-SRV".
    pub domain: String,
    /// Relative target file path, e.g. "src/middleware/auth.ts".
    pub filepath: String,
    /// Causal Git commit or revision anchor, e.g. "v4182" or "a1b2c3d".
    pub base_revision: String,
    /// AST coordinate locating the targeted structural symbol.
    pub coordinate: AstCoordinate,
    /// Ordered sequence of discrete delta operations.
    pub operations: Vec<KdslOperation>,
    /// Architecture invariants and verification checks required for acceptance.
    pub verification_invariants: Vec<String>,
}

impl KdslPacket {
    /// Constructs a new `KdslPacket`.
    pub fn new(
        version: impl Into<String>,
        domain: impl Into<String>,
        filepath: impl Into<String>,
        base_revision: impl Into<String>,
        coordinate: AstCoordinate,
        operations: Vec<KdslOperation>,
        verification_invariants: Vec<String>,
    ) -> Self {
        Self {
            version: version.into(),
            domain: domain.into(),
            filepath: filepath.into(),
            base_revision: base_revision.into(),
            coordinate,
            operations,
            verification_invariants,
        }
    }

    /// Serializes this packet into the dense K-DSL 0.1 wire grammar string.
    pub fn to_wire(&self) -> String {
        let ops_str = self
            .operations
            .iter()
            .map(|op| op.to_wire())
            .collect::<Vec<_>>()
            .join(" ");

        let invariants_str = self.verification_invariants.join(",");

        format!(
            "!δ:{}|{}|{}#{} {} {} !Σ:VERIFY({})",
            self.version,
            self.domain,
            self.filepath,
            self.base_revision,
            self.coordinate,
            ops_str,
            invariants_str
        )
    }

    /// Parses a K-DSL 0.1 wire format string into a `KdslPacket`.
    pub fn parse(wire: &str) -> Result<Self, RouterError> {
        crate::kdsl::parser::parse_kdsl(wire)
    }
}

impl fmt::Display for KdslPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_wire())
    }
}

impl FromStr for KdslPacket {
    type Err = RouterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// AST coordinate identifying the target code symbol without relying on fragile line numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstCoordinate {
    /// Type of AST node (Function, Class, Module, Type, Variable, Custom).
    pub node_type: AstNodeType,
    /// Symbol name or identifier.
    pub identifier: String,
}

impl AstCoordinate {
    /// Constructs a new `AstCoordinate`.
    pub fn new(node_type: AstNodeType, identifier: impl Into<String>) -> Self {
        Self {
            node_type,
            identifier: identifier.into(),
        }
    }
}

impl fmt::Display for AstCoordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "§AST:{}[{}]", self.node_type.as_str(), self.identifier)
    }
}

/// Node type for AST coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AstNodeType {
    /// Function or method declaration (`F`).
    Function,
    /// Class or struct declaration (`C`).
    Class,
    /// Module or namespace declaration (`M`).
    Module,
    /// Type or interface declaration (`T`).
    Type,
    /// Variable or constant declaration (`V`).
    Variable,
    /// Custom AST node discriminator.
    Custom(String),
}

impl AstNodeType {
    /// Returns the canonical short wire representation.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Function => "F",
            Self::Class => "C",
            Self::Module => "M",
            Self::Type => "T",
            Self::Variable => "V",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Parses from a wire representation string.
    pub fn from_wire_str(s: &str) -> Self {
        match s {
            "F" | "Function" => Self::Function,
            "C" | "Class" => Self::Class,
            "M" | "Module" => Self::Module,
            "T" | "Type" => Self::Type,
            "V" | "Variable" => Self::Variable,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl fmt::Display for AstNodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Discrete AST delta operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdslOperation {
    /// The discrete mutation operation code.
    pub opcode: KdslOpcode,
    /// AST path, selector, or anchor for the operation.
    pub target: String,
    /// Replacement AST expression, argument, or content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
}

impl KdslOperation {
    /// Constructs a new operation with both target and replacement.
    pub fn new_with_replacement(
        opcode: KdslOpcode,
        target: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            opcode,
            target: target.into(),
            replacement: Some(replacement.into()),
        }
    }

    /// Constructs a new operation without a replacement (e.g. `DEL_NODE`).
    pub fn new(opcode: KdslOpcode, target: impl Into<String>) -> Self {
        Self {
            opcode,
            target: target.into(),
            replacement: None,
        }
    }

    /// Serializes this operation into wire syntax:
    /// `@OP:<opcode> (<target>)` or `@OP:<opcode> (<target>) -> (<args>)`.
    pub fn to_wire(&self) -> String {
        match &self.replacement {
            Some(repl) => format!(
                "@OP:{} ({}) -> ({})",
                self.opcode.as_str(),
                self.target,
                repl
            ),
            None => format!("@OP:{} ({})", self.opcode.as_str(), self.target),
        }
    }
}

impl fmt::Display for KdslOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_wire())
    }
}

/// Operation code (opcode) for discrete AST transformations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KdslOpcode {
    /// Swap identifier or reference (`SWAP_ID`).
    SwapId,
    /// Set or update property / dictionary entry (`SET_PROP`).
    SetProp,
    /// Insert AST node immediately after target (`INSR_AFT`).
    InsertAfter,
    /// Insert AST node immediately before target (`INSR_BEF`).
    InsertBefore,
    /// Delete specified AST node (`DEL_NODE`).
    DeleteNode,
    /// Replace entire body of function/class/block (`REPLACE_BODY`).
    ReplaceBody,
    /// Fallback raw patch or unified diff block (`RAW_PATCH`).
    RawPatch,
    /// User-defined or experimental opcode.
    Custom(String),
}

impl KdslOpcode {
    /// Returns the canonical uppercase wire representation.
    pub fn as_str(&self) -> &str {
        match self {
            Self::SwapId => "SWAP_ID",
            Self::SetProp => "SET_PROP",
            Self::InsertAfter => "INSR_AFT",
            Self::InsertBefore => "INSR_BEF",
            Self::DeleteNode => "DEL_NODE",
            Self::ReplaceBody => "REPLACE_BODY",
            Self::RawPatch => "RAW_PATCH",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Parses from a wire opcode string.
    pub fn from_wire_str(s: &str) -> Self {
        match s {
            "SWAP_ID" => Self::SwapId,
            "SET_PROP" => Self::SetProp,
            "INSR_AFT" => Self::InsertAfter,
            "INSR_BEF" => Self::InsertBefore,
            "DEL_NODE" => Self::DeleteNode,
            "REPLACE_BODY" => Self::ReplaceBody,
            "RAW_PATCH" => Self::RawPatch,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl fmt::Display for KdslOpcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
