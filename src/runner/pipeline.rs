use anyhow::Ok;
use tokio::{fs::read_to_string, sync::mpsc};
use tokio_util::sync::CancellationToken;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    engine::DockerEngine,
    models::{LogMessage, Pipeline, PipelineReport, RawPipeline, Stage, StageReport, StepReport},
    runner::StageRunner,
};

pub struct PipelineRunner {
    pipeline: Pipeline,
    engine: Arc<DockerEngine>,
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
        let config = read_to_string(pipeline_path).await?;
        let raw: RawPipeline = toml::from_str(&config)?;

        Ok(Self {
            pipeline: raw.compile()?,
            engine,
            cwd: cwd.to_string_lossy().to_string(),
            user: user.into(),
        })
    }

    pub async fn run(self) -> anyhow::Result<PipelineReport> {
        let token = CancellationToken::new();
        let mut stage_reports = Vec::new();
        let mut log_store: HashMap<String, Vec<String>> = HashMap::new();

        let (log_tx, mut log_rx) = mpsc::channel::<LogMessage>(100);

        let log_handle = tokio::spawn(async move {
            while let Some(log) = log_rx.recv().await {
                let prefix = if log.is_error { "ERR" } else { "OUT" };
                let line = format!("[{prefix}] [{}] {}", log.step_name, log.line.trim_end());
                log_store.entry(log.step_name).or_default().push(line);
            }
            log_store
        });

        for stage in self.pipeline.stages.iter() {
            println!("🚀 Starting Stage: {}", stage.name);

            if token.is_cancelled() {
                let skipped_steps = stage
                    .steps
                    .iter()
                    .map(|s| StepReport::skipped(&s.exploded_name))
                    .collect();

                stage_reports.push(StageReport {
                    step_reports: skipped_steps,
                });
                continue;
            }

            self.pre_pull_images(stage).await?;

            let stage_report = StageRunner::new(stage, self.engine.clone(), &self.cwd, &self.user)
                .run(log_tx.clone(), token.clone())
                .await?;

            stage_reports.push(stage_report.clone());

            if !stage_report.is_success() || token.is_cancelled() {
                token.cancel();
                println!("🛑 Pipeline halted due to error in stage '{}'", stage.name);
            }
        }

        drop(log_tx);

        let final_logs = log_handle.await?;

        Ok(PipelineReport {
            stage_reports,
            logs: final_logs,
        })
    }

    async fn pre_pull_images(&self, stage: &Stage) -> anyhow::Result<()> {
        let unique_images: HashSet<String> =
            stage.steps.iter().map(|step| step.image.clone()).collect();

        for img in unique_images {
            println!("🚚 Pre-pulling image: {}", img);
            self.engine.pull_image(&img).await?;
        }

        Ok(())
    }
}
