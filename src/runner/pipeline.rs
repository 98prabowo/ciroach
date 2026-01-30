use anyhow::Ok;
use colored::Colorize;
use futures_util::future::try_join_all;
use tokio_util::sync::CancellationToken;

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use crate::{
    engine::DockerEngine,
    logger::Logger,
    models::{Pipeline, PipelineReport, Stage, StageReport, StepReport},
    runner::StageRunner,
    ui::PreFlightUI,
};

pub struct PipelineRunner {
    pipeline: Pipeline,
    engine: Arc<DockerEngine>,
    cwd: String,
    user: String,
}

impl PipelineRunner {
    pub async fn new(
        pipeline: Pipeline,
        user: impl Into<String>,
        cwd: PathBuf,
    ) -> anyhow::Result<Self> {
        let engine = Arc::new(DockerEngine::new()?);

        Ok(Self {
            pipeline,
            engine,
            cwd: cwd.to_string_lossy().to_string(),
            user: user.into(),
        })
    }

    pub async fn run(self, token: CancellationToken) -> anyhow::Result<PipelineReport> {
        let logger = Logger::new(100);
        let mut stage_reports = Vec::new();

        for stage in self.pipeline.stages.iter() {
            if token.is_cancelled() {
                stage_reports.push(self.skip_stage(stage));
                continue;
            }

            println!("\n-- Stage: {} --", stage.name.to_uppercase().bold());

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

        let ui = Arc::new(PreFlightUI::new(&unique_images));

        let pull_tasks = unique_images.into_iter().map(|img| {
            let engine = self.engine.clone();
            let progress_ui = Arc::clone(&ui);

            tokio::spawn(async move {
                let img_clone = img.clone();
                let finish_ui = Arc::clone(&progress_ui);

                let result = engine
                    .pull_image(&img, move |curr, tot| {
                        progress_ui.update_progress(&img_clone, curr, tot);
                    })
                    .await;

                match result {
                    std::result::Result::Ok(_) => finish_ui.succeed_image(&img),
                    std::result::Result::Err(_) => finish_ui.failed_image(&img),
                }

                result
            })
        });

        try_join_all(pull_tasks).await?;

        println!();

        Ok(())
    }
}
