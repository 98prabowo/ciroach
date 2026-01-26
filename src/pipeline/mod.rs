pub mod config;
pub mod runner;
pub mod types;

pub use config::*;
pub use runner::*;
use tokio::{fs::{File, read_to_string}, io::AsyncWriteExt, sync::mpsc};
use tokio_util::sync::CancellationToken;
pub use types::*;

use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::Arc};

use crate::engine::DockerEngine;

pub struct PipelineRunner {
    engine: Arc<DockerEngine>,
    config: PipelineConfig,
    cwd: String,
    user: String,
}

impl PipelineRunner {
    pub async fn new(
        pipeline_path: impl AsRef<Path>,
        user: impl Into<String>,
        cwd: PathBuf,
    ) -> anyhow::Result<Self> {
        let engine = Arc::new(DockerEngine::new()?);
        let content = read_to_string(pipeline_path).await?;
        let config: PipelineConfig = toml::from_str(&content)?;

        Ok(Self {
            engine,
            config,
            cwd: cwd.to_string_lossy().to_string(),
            user: user.into(),
        })
    }

    pub async fn execute(self, mut log_file: File) -> anyhow::Result<()> {
        let (tx, mut rx) = mpsc::channel::<LogMessage>(100);
        let token = CancellationToken::new();
        let mut handles = vec![];
        let mut log_store: HashMap<String, Vec<String>> = HashMap::new();

        let log_handle = tokio::spawn(async move {
            while let Some(log) = rx.recv().await {
                let prefix = if log.is_error { "ERR" } else { "OUT" };
                let line = format!("[{prefix}] [{}] {}", log.step_name, log.line.trim_end());
                log_store.entry(log.step_name).or_default().push(line);
            }
            log_store
        });

        self.pre_pull_images().await?;

        for step in self.config.pipeline.steps.iter() {
            let runner = StepRunner::new(step.clone(), self.engine.clone(), &self.cwd, &self.user);
            let tx_inner = tx.clone();
            let token_inner = token.clone();

            if step.parallel.unwrap_or(false) {
                handles.push(tokio::spawn(runner.run(tx_inner, token_inner)));
            } else {
                let res = runner.run(tx_inner, token_inner).await;
                handles.push(tokio::spawn(async move { res }));
            }
        }

        drop(tx);

        let mut results = vec![];
        for handle in handles {
            results.push(handle.await?);
        }
        let final_logs = log_handle.await?;

        self.print_report(results, final_logs, &mut log_file)
            .await?;
        Ok(())
    }

    async fn pre_pull_images(&self) -> anyhow::Result<()> {
        let unique_images: HashSet<String> = self
            .config
            .pipeline
            .steps
            .iter()
            .map(|step| step.image.clone())
            .collect();

        for img in unique_images {
            println!("🚚 Pre-pulling image: {}", img);
            self.engine.pull_image(&img).await?;
        }

        Ok(())
    }

    async fn print_report(
        &self,
        results: Vec<StepResult>,
        logs: HashMap<String, Vec<String>>,
        file: &mut File,
    ) -> anyhow::Result<()> {
        Self::write_line(file, "\n--- 📖 Ordered Pipeline Logs ---").await?;

        for step in self.config.pipeline.steps.iter() {
            if let Some(lines) = logs.get(&step.name) {
                Self::write_line(file, &format!("\n=== {} ===", step.name.to_uppercase())).await?;
                for line in lines {
                    Self::write_line(file, line).await?;
                }
            }
        }

        Self::write_line(file, "\n--- 🪳 Summary ---").await?;

        let mut all_success = true;
        for res in results {
            let icon = match res.status {
                StepStatus::Success => "✅",
                StepStatus::Failed => {
                    all_success = false;
                    "❌"
                }
                StepStatus::Skipped => "⏭️",
            };
            Self::write_line(file, &format!("{icon} {}: {}", res.name, res.elapsed)).await?;
        }

        file.flush().await?;

        if !all_success {
            anyhow::bail!("Pipeline failed")
        }

        Ok(())
    }

    async fn write_line(file: &mut File, text: &str) -> anyhow::Result<()> {
        println!("{text}");
        file.write_all(format!("{text}\n").as_bytes()).await?;
        Ok(())
    }
}
