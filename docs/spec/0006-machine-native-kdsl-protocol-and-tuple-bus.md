# SPEC-0006: Machine-Native Protocols, K-DSL 0.1 Symbolic Grammar, and Linda Tuple Bus

- **Status**: Draft / Ready for Implementation
- **Target**: `k0maru-agent-router` v0.2.0 (Phase 6)
- **Author**: K0maru Architecture Team
- **Date**: 2026-10-13
- **Related Documents**:
  - `SecondBrain/01_AI_Logs/2026-10-13-machine-native-protocols-and-k-dsl-architecture.md`
  - `SecondBrain/99_Attachments/Machine-Native Multi-Agent Communication Protocols.md`

---

## 1. Problem Statement & Background

Current multi-agent interaction relies heavily on unstructured natural language (English/Chinese dialogue). This introduces severe overhead:
1. **Token Bloat & Latency**: Generating natural language incurs 200+ tokens and multi-second autoregressive decoding per hop.
2. **Context Window Exhaustion & Attention Drift**: Chat transcripts expand quadratically ($O(N^2)$), causing instructions to blur and leading to syntax errors or hallucinations.
3. **The Tower of Babel & Compaction Amnesia**: If agents negotiate dynamic ad-hoc dialects, keeping those schemas in the Master LLM's context window consumes memory and causes catastrophic amnesia when `/compact` or context truncations occur.

To resolve the **Agent Communication Trilemma** (Versatility, Efficiency, Portability) across both proprietary closed-source models (Claude 3.5/3.7, GPT-4o, Gemini 2.5) and open-source models:
- **Internal Domain (K0maru Fabric)** must enforce a unified, deterministic machine dialect: **K-DSL 0.1**.
- **Protocol State must be offloaded to the Rust host runtime (`k0maru-router`)**, keeping the LLM's context clean and immune to compaction.
- **Message passing is replaced by a decoupled Linda Tuple-Space** (`out/in/rd` primitives) over filesystem buses, eliminating synchronous conversational blocking.

---

## 2. K-DSL 0.1 Wire Grammar Specification

### 2.1 Formal Grammar (EBNF)

```ebnf
packet         ::= header " " coordinate " " operations " " verification ;
header         ::= "!δ:" version "|" domain "|" filepath "#" revision ;
version        ::= [0-9]+ "." [0-9]+ ;
domain         ::= [A-Z0-9_-]+ ;
filepath       ::= [a-zA-Z0-9_./-]+ ;
revision       ::= [a-zA-Z0-9_-]+ ;

coordinate     ::= "§AST:" node_type "[" identifier "]" ;
node_type      ::= "F" | "C" | "M" | "T" | "V" ; (* Function, Class, Module, Type, Variable *)
identifier     ::= [a-zA-Z0-9_]+ ;

operations     ::= operation (" " operation)* ;
operation      ::= "@OP:" opcode "(" target ")" (" -> " "(" args ")")? ;
opcode         ::= "SWAP_ID" | "SET_PROP" | "INSR_AFT" | "INSR_BEF" | "DEL_NODE" | "REPLACE_BODY" | "RAW_PATCH" ;
target         ::= [^\)]+ ;
args           ::= [^\)]+ ;

verification   ::= "!Σ:VERIFY(" invariant_list ")" ;
invariant_list ::= invariant ("," invariant)* ;
invariant      ::= [A-Z0-9_#-]+ ;
```

### 2.2 Wire Example

```text
!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182 §AST:F[verifyToken] @OP:SWAP_ID (Call[jwt.verify].Arg[1]) -> (Call[getPublicKey]) @OP:SET_PROP (Call[jwt.verify].Arg[2].Prop[algorithms]) -> (["RS256"]) @OP:INSR_AFT (Call[jwt.verify]) -> (Await[Call[checkRevocation(decoded.jti)]]) !Σ:VERIFY(TS_COMPILE,LIVING_ADR#SEC-004)
```

- **Header**: `!δ:0.1|AUTH-SRV|src/middleware/auth.ts#v4182` establishes causal order and target file.
- **Coordinate**: `§AST:F[verifyToken]` anchors the AST node without relying on fragile line numbers.
- **Operations**: RFC 6902-inspired discrete delta operations (`@OP:SWAP_ID`, `@OP:SET_PROP`, `@OP:INSR_AFT`).
- **Verification**: `!Σ:VERIFY(TS_COMPILE, LIVING_ADR#SEC-004)` enforces that the change is accepted only if `k0maru-verifier` and `k0maru-memory` Living ADR guard pass.

### 2.3 Token Budget & Compression Target
- Baseline English CoT: ~260 tokens
- Baseline JSON-RPC: ~145 tokens
- **K-DSL 0.1 Target**: `< 50 tokens` (empirical ~38 tokens, >80% reduction)

---

## 3. Data Structures & Module Architecture

### 3.1 `src/kdsl/` Module
- `mod.rs`: Public re-exports and high-level encoder/decoder.
- `parser.rs`: Deterministic Nom/Regex/Custom zero-copy parser for K-DSL wire strings.
- `model.rs`:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct KdslPacket {
      pub version: String,
      pub domain: String,
      pub filepath: String,
      pub base_revision: String,
      pub coordinate: AstCoordinate,
      pub operations: Vec<KdslOperation>,
      pub verification_invariants: Vec<String>,
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct AstCoordinate {
      pub node_type: AstNodeType,
      pub identifier: String,
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub enum AstNodeType {
      Function,
      Class,
      Module,
      Type,
      Variable,
      Custom(String),
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct KdslOperation {
      pub opcode: KdslOpcode,
      pub target: String,
      pub replacement: Option<String>,
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub enum KdslOpcode {
      SwapId,
      SetProp,
      InsertAfter,
      InsertBefore,
      DeleteNode,
      ReplaceBody,
      RawPatch,
      Custom(String),
  }
  ```
- `bridge.rs`: Bidirectional conversion between `TaskEnvelope` and `KdslPacket`.
- `prompt.rs`: System prompt contract generator enforcing closed-source models (Claude/Gemini/GPT-4o) to output valid K-DSL without conversational boilerplate.

### 3.2 `src/tuple_space/` Module (Linda Tuple-Space)
Extends signals and receipts into associative shared memory:
- `mod.rs`: `TupleSpaceEngine`
- `tuple.rs`:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
  pub struct Tuple {
      pub id: String,
      pub kind: String,        // e.g. "TASK_READY", "TASK_CLAIMED", "TASK_COMPLETED", "TASK_FAILED"
      pub domain: String,      // e.g. "AUTH-SRV", "SecondBrain"
      pub task_id: String,
      pub payload: String,     // JSON string or K-DSL wire string
      pub created_at: String,
      pub ttl_secs: Option<u64>,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct TuplePattern {
      pub kind: Option<String>,
      pub domain: Option<String>,
      pub task_id: Option<String>,
  }
  ```
- Operations:
  - `out(tuple)`: Writes atomic `.json` tuple into `~/.k0maru/tuples/` with file lock.
  - `in(pattern)`: Finds matching tuple, claims (deletes/moves) it atomically, returns it.
  - `rd(pattern)`: Non-destructively reads first matching tuple.
  - `list(pattern)`: Lists all matching tuples without consumption.

---

## 4. CLI Surface & Integration

```bash
# K-DSL Subcommands
k0maru-router kdsl encode <envelope.json>
k0maru-router kdsl decode "<wire_string>"
k0maru-router kdsl contract --json

# Tuple-Space Subcommands
k0maru-router tuple out --kind TASK_READY --domain AUTH-SRV --task-id t1 --payload "<wire>"
k0maru-router tuple in --kind TASK_READY --domain AUTH-SRV
k0maru-router tuple rd --kind TASK_READY
k0maru-router tuple list --domain AUTH-SRV
```

---

## 5. Non-Functional Invariants
1. **Zero External Daemon**: Runs purely in-process via static binary and local filesystem lock (`~/.k0maru/tuples`).
2. **Speed & Allocation**: K-DSL parsing must take `< 500µs` and require zero allocations beyond parsed structs.
3. **Compatibility**: MSRV Rust 2021, all tests pass `cargo test`, `cargo clippy -- -D warnings` zero warnings.
