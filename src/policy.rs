use crate::envelope::TaskEnvelope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    TrustedInternal,
    UntrustedExternal { reason: String },
}

pub struct BoundaryPolicy;

impl BoundaryPolicy {
    pub fn evaluate(envelope: &TaskEnvelope) -> PolicyVerdict {
        if envelope.target.is_external {
            PolicyVerdict::UntrustedExternal {
                reason: format!(
                    "Target '{}' is flagged as external 3rd-party OSS",
                    envelope.target.identifier
                ),
            }
        } else {
            PolicyVerdict::TrustedInternal
        }
    }
}
