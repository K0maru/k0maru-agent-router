use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Working,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pane_id: String,
    pub cwd: String,
    pub status: SessionStatus,
}

pub trait MultiplexerDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn list_sessions(&self) -> Result<Vec<LiveSession>>;
    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<()>;
    fn spawn_pane(&self, workspace: &Path) -> Result<LiveSession>;
}
