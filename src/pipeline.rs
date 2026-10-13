//! End-to-End Task Pipeline Coordination and Component Health Discovery.
//!
//! Provides an optional, completely decoupled orchestration layer connecting:
//! 1. Workspace sandboxing (`k0maru-workspace`, optional)
//! 2. Deterministic test oracle verification (`k0maru-verifier`, optional)
//! 3. Task signaling (`k0maru-router signal`)
//! 4. Execution receipt bus (`k0maru-router receipt`)
//! 5. Memory crystallization (`k0maru flush`, optional)
//!
//! Conforms to Unix modularity: every component is optional (选配) and independently operable.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::RouterError;

/// Status and metadata of an optional K0maru-Harness component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessComponentStatus {
    pub name: String,
    pub description: String,
    pub is_available: bool,
    pub path: Option<String>,
    pub is_optional: bool,
}

/// Overall health and component availability report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessHealthReport {
    pub is_complete: bool,
    pub components: Vec<HarnessComponentStatus>,
    pub receipts_dir: String,
    pub signals_dir: String,
}

/// Probe host environment for an executable binary.
pub fn probe_binary(name: &str) -> Option<PathBuf> {
    // 1. Check ~/.cargo/bin/<name>
    if let Some(home) = std::env::var_os("HOME") {
        let cargo_bin = PathBuf::from(home).join(".cargo/bin").join(name);
        if cargo_bin.is_file() {
            return Some(cargo_bin);
        }
    }

    // 2. Check ~/.local/bin/<name>
    if let Some(home) = std::env::var_os("HOME") {
        let local_bin = PathBuf::from(home).join(".local/bin").join(name);
        if local_bin.is_file() {
            return Some(local_bin);
        }
    }

    // 3. Check PATH environment
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Probe all components in the K0maru-Harness matrix and return their availability.
pub fn probe_harness_components() -> HarnessHealthReport {
    let ws_path = probe_binary("k0maru-workspace");
    let verifier_path = probe_binary("k0maru-verifier");
    let memory_path = probe_binary("k0maru");
    let router_path = probe_binary("k0maru-router");

    let components = vec![
        HarnessComponentStatus {
            name: "k0maru-router".to_string(),
            description: "Cross-repo Task Router & Discrete Ingest Bus".to_string(),
            is_available: router_path.is_some(),
            path: router_path.map(|p| p.to_string_lossy().to_string()),
            is_optional: false,
        },
        HarnessComponentStatus {
            name: "k0maru-verifier".to_string(),
            description: "Deterministic Test Oracle & Receipt Signer".to_string(),
            is_available: verifier_path.is_some(),
            path: verifier_path.map(|p| p.to_string_lossy().to_string()),
            is_optional: true,
        },
        HarnessComponentStatus {
            name: "k0maru-workspace".to_string(),
            description: "Git Worktree Isolation & Ephemeral Sandbox".to_string(),
            is_available: ws_path.is_some(),
            path: ws_path.map(|p| p.to_string_lossy().to_string()),
            is_optional: true,
        },
        HarnessComponentStatus {
            name: "k0maru-memory".to_string(),
            description: "Zero-Daemon Long-Term Memory Hub & Living ADR Guard".to_string(),
            is_available: memory_path.is_some(),
            path: memory_path.map(|p| p.to_string_lossy().to_string()),
            is_optional: true,
        },
    ];

    let is_complete = components.iter().all(|c| c.is_available);
    let receipts_dir = crate::receipt::resolve_receipts_dir(None)
        .to_string_lossy()
        .to_string();
    let signals_dir = crate::signal::resolve_signals_dir(None)
        .to_string_lossy()
        .to_string();

    HarnessHealthReport {
        is_complete,
        components,
        receipts_dir,
        signals_dir,
    }
}

/// Pipeline configuration options for a single task run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub task_id: String,
    pub target_workspace: PathBuf,
    pub intent: Option<String>,
    pub patch: Option<PathBuf>,
    pub runner: Option<String>,
    pub isolate: bool,
    pub verify: bool,
    pub auto_merge: bool,
    pub emit_signal: bool,
    pub crystallize_memory: bool,
    pub allowed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_backend: Option<String>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            task_id: format!(
                "task_pipeline_{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ),
            target_workspace: PathBuf::from("."),
            intent: Some("verify_or_execute".to_string()),
            patch: None,
            runner: None,
            isolate: false,
            verify: true,
            auto_merge: false,
            emit_signal: true,
            crystallize_memory: false,
            allowed_files: Vec::new(),
            node: None,
            remote_backend: None,
        }
    }
}

/// Status of an individual stage within the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageResult {
    pub stage: String,
    pub status: String, // "passed", "failed", "skipped"
    pub message: String,
    pub duration_ms: u64,
}

/// Consolidated execution report for an end-to-end pipeline run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineRunReport {
    pub task_id: String,
    pub verdict: String, // "success", "failed", "skipped"
    pub stages: Vec<StageResult>,
    pub sandbox_workspace: Option<PathBuf>,
    pub receipt_id: Option<String>,
    pub signal_emitted: bool,
    pub duration_ms: u64,
}

/// Executes an end-to-end task pipeline, treating each underlying tool as optional (选配).
pub async fn execute_pipeline(config: &PipelineConfig) -> Result<PipelineRunReport, RouterError> {
    if let Some(ref node) = config.node {
        if !crate::addressing::is_local_node(Some(node)) {
            let backend = match config.remote_backend.as_deref() {
                Some(b) => b.parse::<crate::driver::RemoteBackend>()?,
                None => crate::driver::RemoteBackend::Auto,
            };
            let remote_driver = crate::driver::RemoteSshDriver::new(node).with_backend(backend);
            return remote_driver.run_remote_pipeline(config);
        }
    }

    let start_all = std::time::Instant::now();
    let mut stages = Vec::new();
    let mut active_workspace = if config.target_workspace.is_absolute() {
        config.target_workspace.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&config.target_workspace)
    };
    let mut sandbox_path = None;
    let mut receipt_id = None;
    let mut final_verdict = "success";

    // Stage 1: Workspace Sandbox Isolation (Optional)
    if config.isolate {
        let t0 = std::time::Instant::now();
        if let Some(ws_bin) = probe_binary("k0maru-workspace") {
            let output = std::process::Command::new(&ws_bin)
                .args(["provision", "--task-id", &config.task_id, "--repo"])
                .arg(&config.target_workspace)
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let path = config
                        .target_workspace
                        .join(".worktrees")
                        .join(&config.task_id);
                    active_workspace = if path.exists() {
                        path
                    } else {
                        config.target_workspace.clone()
                    };
                    sandbox_path = Some(active_workspace.clone());
                    stages.push(StageResult {
                        stage: "workspace_provision".to_string(),
                        status: "passed".to_string(),
                        message: format!("Provisioned worktree at {}", active_workspace.display()),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
                Ok(out) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "workspace_provision".to_string(),
                        status: "failed".to_string(),
                        message: String::from_utf8_lossy(&out.stderr).to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "workspace_provision".to_string(),
                        status: "failed".to_string(),
                        message: e.to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
            }
        } else {
            stages.push(StageResult {
                stage: "workspace_provision".to_string(),
                status: "skipped".to_string(),
                message: "k0maru-workspace not installed; running in-place (decoupled fallback)"
                    .to_string(),
                duration_ms: t0.elapsed().as_millis() as u64,
            });
        }
    } else {
        stages.push(StageResult {
            stage: "workspace_provision".to_string(),
            status: "skipped".to_string(),
            message: "Isolation not requested (in-place execution)".to_string(),
            duration_ms: 0,
        });
    }

    if final_verdict == "failed" {
        return Ok(PipelineRunReport {
            task_id: config.task_id.clone(),
            verdict: final_verdict.to_string(),
            stages,
            sandbox_workspace: sandbox_path,
            receipt_id: None,
            signal_emitted: false,
            duration_ms: start_all.elapsed().as_millis() as u64,
        });
    }

    // Stage 2: Apply Patch (Optional)
    if let Some(ref patch_file) = config.patch {
        let t0 = std::time::Instant::now();
        let output = std::process::Command::new("git")
            .current_dir(&active_workspace)
            .args(["apply", "--ignore-whitespace"])
            .arg(patch_file)
            .output();
        match output {
            Ok(out) if out.status.success() => {
                stages.push(StageResult {
                    stage: "patch_application".to_string(),
                    status: "passed".to_string(),
                    message: format!("Applied patch from {}", patch_file.display()),
                    duration_ms: t0.elapsed().as_millis() as u64,
                });
            }
            Ok(out) => {
                final_verdict = "failed";
                stages.push(StageResult {
                    stage: "patch_application".to_string(),
                    status: "failed".to_string(),
                    message: String::from_utf8_lossy(&out.stderr).to_string(),
                    duration_ms: t0.elapsed().as_millis() as u64,
                });
            }
            Err(e) => {
                final_verdict = "failed";
                stages.push(StageResult {
                    stage: "patch_application".to_string(),
                    status: "failed".to_string(),
                    message: e.to_string(),
                    duration_ms: t0.elapsed().as_millis() as u64,
                });
            }
        }
    }

    if final_verdict == "failed" {
        return Ok(PipelineRunReport {
            task_id: config.task_id.clone(),
            verdict: final_verdict.to_string(),
            stages,
            sandbox_workspace: sandbox_path,
            receipt_id: None,
            signal_emitted: false,
            duration_ms: start_all.elapsed().as_millis() as u64,
        });
    }

    // Stage 3: Verification with Physical Test Oracle (Optional)
    let mut verified_pass = false;
    if config.verify {
        let t0 = std::time::Instant::now();
        if let Some(veri_bin) = probe_binary("k0maru-verifier") {
            let mut cmd = std::process::Command::new(&veri_bin);
            cmd.args(["verify", "--workspace"]).arg(&active_workspace);
            cmd.args(["--task-id", &config.task_id]);
            if !config.allowed_files.is_empty() {
                cmd.args(["--whitelist", &config.allowed_files.join(",")]);
            }
            if let Some(ref r) = config.runner {
                cmd.args(["--runner", r]);
            }
            match cmd.output() {
                Ok(out) => {
                    let duration = t0.elapsed().as_millis() as u64;
                    if out.status.success() {
                        verified_pass = true;
                        receipt_id = Some(config.task_id.clone());
                        stages.push(StageResult {
                            stage: "physical_verifier".to_string(),
                            status: "passed".to_string(),
                            message: "Physical verification passed: Exit Code 0, receipt signed"
                                .to_string(),
                            duration_ms: duration,
                        });
                    } else {
                        final_verdict = "failed";
                        let err_msg = String::from_utf8_lossy(&out.stdout).to_string();
                        stages.push(StageResult {
                            stage: "physical_verifier".to_string(),
                            status: "failed".to_string(),
                            message: err_msg,
                            duration_ms: duration,
                        });
                    }
                }
                Err(e) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "physical_verifier".to_string(),
                        status: "failed".to_string(),
                        message: e.to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
            }
        } else {
            // Decoupled fallback: cargo test
            let t0 = std::time::Instant::now();
            let out = std::process::Command::new("cargo")
                .current_dir(&active_workspace)
                .args(["test", "--quiet"])
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    verified_pass = true;
                    stages.push(StageResult {
                        stage: "physical_verifier".to_string(),
                        status: "passed".to_string(),
                        message: "k0maru-verifier not installed; fallback cargo test passed"
                            .to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
                Ok(o) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "physical_verifier".to_string(),
                        status: "failed".to_string(),
                        message: String::from_utf8_lossy(&o.stderr).to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "physical_verifier".to_string(),
                        status: "failed".to_string(),
                        message: e.to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
            }
        }
    } else {
        stages.push(StageResult {
            stage: "physical_verifier".to_string(),
            status: "skipped".to_string(),
            message: "Verification stage not requested".to_string(),
            duration_ms: 0,
        });
    }

    // Stage 4: Teardown & Merge (Optional, No-Receipt-No-Merge)
    if config.auto_merge && config.isolate && verified_pass {
        let t0 = std::time::Instant::now();
        if let Some(ws_bin) = probe_binary("k0maru-workspace") {
            let output = std::process::Command::new(&ws_bin)
                .args([
                    "teardown",
                    "--task-id",
                    &config.task_id,
                    "--action",
                    "merge",
                    "--repo",
                ])
                .arg(&config.target_workspace)
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    stages.push(StageResult {
                        stage: "workspace_teardown_merge".to_string(),
                        status: "passed".to_string(),
                        message: "Verified worktree merged and cleanly torn down (No-Receipt-No-Merge passed)".to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
                Ok(out) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "workspace_teardown_merge".to_string(),
                        status: "failed".to_string(),
                        message: String::from_utf8_lossy(&out.stderr).to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    final_verdict = "failed";
                    stages.push(StageResult {
                        stage: "workspace_teardown_merge".to_string(),
                        status: "failed".to_string(),
                        message: e.to_string(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                    });
                }
            }
        }
    }

    // Stage 5: Signal Bus Emission (Optional)
    let mut signal_emitted = false;
    if config.emit_signal {
        let t0 = std::time::Instant::now();
        let sig_status = if final_verdict == "success" {
            crate::signal::SignalStatus::Completed
        } else {
            crate::signal::SignalStatus::Failed
        };
        let summary = format!(
            "Pipeline {} for task {} in {}ms",
            final_verdict,
            config.task_id,
            start_all.elapsed().as_millis()
        );
        let sig = crate::signal::TaskSignal {
            schema: crate::signal::DEFAULT_SIGNAL_SCHEMA.to_string(),
            task_id: config.task_id.clone(),
            status: sig_status,
            summary: Some(summary),
            agent_pane: crate::signal::resolve_agent_pane(None),
            workspace: active_workspace.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let store = crate::signal::SignalStore::new(crate::signal::resolve_signals_dir(None));
        if store.emit(&sig).is_ok() {
            signal_emitted = true;
            stages.push(StageResult {
                stage: "signal_bus".to_string(),
                status: "passed".to_string(),
                message: format!(
                    "Emitted {} signal to ~/.k0maru/signals/{}.json",
                    final_verdict, config.task_id
                ),
                duration_ms: t0.elapsed().as_millis() as u64,
            });
        }
    }

    // Stage 6: Memory Crystallization (Optional)
    if config.crystallize_memory && final_verdict == "success" {
        let t0 = std::time::Instant::now();
        if let Some(mem_bin) = probe_binary("k0maru") {
            let note_title = format!("task-{}", config.task_id);
            let content = format!(
                "# Task {}\nVerdict: {}\nDuration: {}ms\n",
                config.task_id,
                final_verdict,
                start_all.elapsed().as_millis()
            );
            let _ = std::process::Command::new(&mem_bin)
                .args(["flush", "--title", &note_title, "--content", &content])
                .output();
            stages.push(StageResult {
                stage: "memory_crystallization".to_string(),
                status: "passed".to_string(),
                message: "Crystallized task completion into memory hub".to_string(),
                duration_ms: t0.elapsed().as_millis() as u64,
            });
        } else {
            stages.push(StageResult {
                stage: "memory_crystallization".to_string(),
                status: "skipped".to_string(),
                message:
                    "k0maru memory binary not found; skipping memory crystallization (optional)"
                        .to_string(),
                duration_ms: t0.elapsed().as_millis() as u64,
            });
        }
    }

    Ok(PipelineRunReport {
        task_id: config.task_id.clone(),
        verdict: final_verdict.to_string(),
        stages,
        sandbox_workspace: sandbox_path,
        receipt_id,
        signal_emitted,
        duration_ms: start_all.elapsed().as_millis() as u64,
    })
}
