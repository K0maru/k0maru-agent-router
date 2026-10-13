//! Boundary Policy Engine evaluating target trust boundaries.
//!
//! Enforces architecture anti-corruption and permission isolation by determining
//! whether a target repository is trusted internal or an untrusted external
//! 3rd-party open-source dependency.

use crate::envelope::{TargetInfo, TaskEnvelope};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents the trust boundary evaluation outcome for a task target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyVerdict {
    /// Trusted internal target repository: permitted for automated routing.
    TrustedInternal,
    /// Untrusted external 3rd-party OSS dependency: must be quarantined in HITL Outbox.
    UntrustedExternal {
        /// Explanatory justification referencing the external target identifier.
        reason: String,
    },
}

impl PolicyVerdict {
    /// Returns `true` if the target is internal and trusted for automated dispatch.
    pub fn is_trusted(&self) -> bool {
        matches!(self, Self::TrustedInternal)
    }

    /// Returns `true` if the target is external and must be quarantined in HITL Outbox.
    pub fn is_untrusted(&self) -> bool {
        matches!(self, Self::UntrustedExternal { .. })
    }

    /// Returns the quarantine reason string if untrusted, or `None` if trusted.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::UntrustedExternal { reason } => Some(reason.as_str()),
            Self::TrustedInternal => None,
        }
    }
}

impl fmt::Display for PolicyVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TrustedInternal => write!(f, "TrustedInternal"),
            Self::UntrustedExternal { reason } => write!(f, "UntrustedExternal ({reason})"),
        }
    }
}

/// Boundary Policy Engine evaluating target trust boundaries.
pub struct BoundaryPolicy;

impl BoundaryPolicy {
    /// Evaluates a `TaskEnvelope` against trust boundary invariants.
    ///
    /// - If `target.is_external == false` -> `PolicyVerdict::TrustedInternal`
    /// - If `target.is_external == true`  -> `PolicyVerdict::UntrustedExternal`
    pub fn evaluate(envelope: &TaskEnvelope) -> PolicyVerdict {
        Self::evaluate_target(&envelope.target)
    }

    /// Evaluates target metadata directly against trust boundary invariants.
    pub fn evaluate_target(target: &TargetInfo) -> PolicyVerdict {
        if target.is_external {
            PolicyVerdict::UntrustedExternal {
                reason: format!(
                    "Target '{}' is flagged as external 3rd-party OSS",
                    target.identifier
                ),
            }
        } else {
            PolicyVerdict::TrustedInternal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{CallerInfo, TaskPayload};

    fn make_test_envelope(identifier: &str, is_external: bool) -> TaskEnvelope {
        TaskEnvelope {
            schema: None,
            id: "task_test_1".to_string(),
            created_at: "2026-10-11T10:00:00Z".to_string(),
            caller: CallerInfo {
                workspace: "/test/ws".to_string(),
                agent: "test_agent".to_string(),
                pane_id: None,
                node: None,
            },
            target: TargetInfo {
                identifier: identifier.to_string(),
                workspace: if is_external {
                    None
                } else {
                    Some("/internal/ws".to_string())
                },
                is_external,
                node: None,
            },
            intent: "bug_report".to_string(),
            payload: TaskPayload {
                title: "Test Bug".to_string(),
                severity: "high".to_string(),
                reproduction: None,
                expected: None,
            },
        }
    }

    #[test]
    fn test_internal_target_evaluates_to_trusted() {
        let envelope = make_test_envelope("k0maru-agent-memory", false);
        let verdict = BoundaryPolicy::evaluate(&envelope);

        assert_eq!(verdict, PolicyVerdict::TrustedInternal);
        assert!(verdict.is_trusted());
        assert!(!verdict.is_untrusted());
        assert_eq!(verdict.reason(), None);
    }

    #[test]
    fn test_external_target_evaluates_to_untrusted() {
        let envelope = make_test_envelope("react-dom", true);
        let verdict = BoundaryPolicy::evaluate(&envelope);

        assert!(verdict.is_untrusted());
        assert!(!verdict.is_trusted());
        let reason = verdict.reason().expect("Reason must be present");
        assert!(reason.contains("react-dom"));
        assert!(reason.contains("external 3rd-party OSS"));
    }

    #[test]
    fn test_evaluate_target_directly() {
        let internal_target = TargetInfo::new_internal("core-crate", "/path/to/core");
        assert_eq!(
            BoundaryPolicy::evaluate_target(&internal_target),
            PolicyVerdict::TrustedInternal
        );

        let external_target = TargetInfo::new_external("serde");
        let verdict = BoundaryPolicy::evaluate_target(&external_target);
        assert!(verdict.is_untrusted());
        assert!(verdict
            .reason()
            .expect("Reason must exist")
            .contains("serde"));
    }

    #[test]
    fn test_verdict_display() {
        assert_eq!(
            PolicyVerdict::TrustedInternal.to_string(),
            "TrustedInternal"
        );
        let untrusted = PolicyVerdict::UntrustedExternal {
            reason: "Target 'lodash' is external".to_string(),
        };
        assert_eq!(
            untrusted.to_string(),
            "UntrustedExternal (Target 'lodash' is external)"
        );
    }

    #[test]
    fn test_verdict_serde_roundtrip() {
        let trusted = PolicyVerdict::TrustedInternal;
        let json = serde_json::to_string(&trusted).unwrap();
        let deserialized: PolicyVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(trusted, deserialized);

        let untrusted = PolicyVerdict::UntrustedExternal {
            reason: "Untrusted OSS".to_string(),
        };
        let json = serde_json::to_string(&untrusted).unwrap();
        let deserialized: PolicyVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(untrusted, deserialized);
    }

    #[test]
    fn test_target_with_special_characters_and_unicode() {
        let envelope = make_test_envelope("开源组件 🚀 <v2.0>", true);
        let verdict = BoundaryPolicy::evaluate(&envelope);
        assert!(verdict.is_untrusted());
        let reason = verdict.reason().unwrap();
        assert!(reason.contains("开源组件 🚀 <v2.0>"));
    }
}
