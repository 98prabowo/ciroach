use anyhow::Ok;
use futures_util::future::try_join_all;
use tokio::fs::read_to_string;
use tokio_util::sync::CancellationToken;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use crate::{
    engine::DockerEngine,
    logger::Logger,
    models::{Pipeline, PipelineReport, RawPipeline, Stage, StageReport, StepReport},
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
        let logger = Logger::new(100);
        let mut stage_reports = Vec::new();

        for stage in self.pipeline.stages.iter() {
            if token.is_cancelled() {
                stage_reports.push(self.skip_stage(stage));
                continue;
            }

            println!("🚀 Starting Stage: {}", stage.name);
            self.pre_pull_images(stage).await?;

            let runner = StageRunner::new(stage, self.engine.clone(), &self.cwd, &self.user);
            let report = runner.run(logger.tx(), token.clone()).await?;

            stage_reports.push(report.clone());

            if !report.is_success() {
                token.cancel();
                println!("🛑 Pipeline halted due to error in stage '{}'", stage.name);
            }
        }

        let final_logs = logger.finish().await?;

        Ok(PipelineReport {
            stage_reports,
            logs: final_logs,
        })
    }

    fn skip_stage(&self, stage: &Stage) -> StageReport {
        StageReport {
            step_reports: stage
                .steps
                .iter()
                .map(|step| StepReport::skipped(&step.exploded_name))
                .collect(),
        }
    }

    async fn pre_pull_images(&self, stage: &Stage) -> anyhow::Result<()> {
        let unique_images: HashSet<String> =
            stage.steps.iter().map(|step| step.image.clone()).collect();

        if unique_images.is_empty() {
            return Ok(());
        }

        let start = Instant::now();

        let pull_tasks = unique_images.iter().map(|img| {
            println!("🚚 Pre-pulling image: {}", img);
            self.engine.pull_image(img)
        });

        try_join_all(pull_tasks).await?;

        println!(
            "📥 All images pulled in {:.2}s (Total images: {})",
            start.elapsed().as_secs_f64(),
            unique_images.len()
        );

        Ok(())
    }
}
