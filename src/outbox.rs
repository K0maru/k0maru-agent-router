use crate::envelope::TaskEnvelope;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct HitlOutbox;

impl HitlOutbox {
    pub fn stage(envelope: &TaskEnvelope, outbox_dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(outbox_dir)
            .with_context(|| format!("Failed to create outbox directory {:?}", outbox_dir))?;

        let file_path = outbox_dir.join(format!("issue_{}.md", envelope.id));
        let content = format!(
            "# Staged Issue Draft: {}\n\n- **ID**: {}\n- **Target**: {}\n- **Severity**: {}\n- **Created At**: {}\n\n## Reproduction\n```json\n{}\n```\n\n## Expected\n{}\n\n---\n*Status: Pending Human Authorization (HITL)*\n",
            envelope.payload.title,
            envelope.id,
            envelope.target.identifier,
            envelope.payload.severity,
            envelope.created_at,
            serde_json::to_string_pretty(&envelope.payload.reproduction.clone().unwrap_or_default())?,
            envelope.payload.expected.as_deref().unwrap_or("N/A")
        );

        std::fs::write(&file_path, content)?;
        Ok(file_path)
    }
}
