use std::{collections::HashSet, sync::Arc};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    engine::DockerEngine,
    models::{LogMessage, Stage, StageReport, Step, StepReport},
    runner::StepRunner,
};

pub struct StageRunner<'s> {
    stage: &'s Stage,
    engine: Arc<DockerEngine>,
    cwd: String,
    user: String,
}

impl<'s> StageRunner<'s> {
    pub fn new(
        stage: &'s Stage,
        engine: Arc<DockerEngine>,
        cwd: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            engine,
            cwd: cwd.into(),
            user: user.into(),
        }
    }

    pub async fn run(
        &self,
        log_tx: mpsc::Sender<LogMessage>,
        token: CancellationToken,
    ) -> anyhow::Result<StageReport> {
        let mut step_reports = Vec::new();
        let mut completed_step_names = HashSet::new();
        let mut started_step_names = HashSet::new();

        let (status_tx, mut status_rx) = mpsc::channel::<StepReport>(100);
        let total_steps = self.stage.steps.len();

        while completed_step_names.len() < total_steps {
            if !token.is_cancelled() {
                for step in self.stage.steps.iter() {
                    if started_step_names.contains(&step.exploded_name) {
                        continue;
                    }

                    if self.can_start(step, &completed_step_names) {
                        started_step_names.insert(step.exploded_name.clone());

                        let runner = StepRunner::new(
                            step.clone(),
                            self.engine.clone(),
                            &self.cwd,
                            &self.user,
                        );
                        let log_tx_inner = log_tx.clone();
                        let status_tx_inner = status_tx.clone();
                        let token_inner = token.clone();

                        tokio::spawn(async move {
                            let result = runner.run(log_tx_inner, token_inner).await;
                            status_tx_inner.send(result).await.ok();
                        });
                    }
                }
            }

            if started_step_names.len() > completed_step_names.len()
                && let Some(report) = status_rx.recv().await
            {
                completed_step_names.insert(report.name.clone());
                step_reports.push(report);

                // Immediately check if we can start more steps or if we need to drain
                continue;
            }

            // If the token was cancelled, and we have received reports for everything we started,
            // we can safely stop the loop even if some steps in the stage never ran.
            if token.is_cancelled() && started_step_names.len() == completed_step_names.len() {
                break;
            }

            // If we aren't cancelled, but nothing is running and we aren't finished, it's a deadlock.
            if !token.is_cancelled() && started_step_names.len() == completed_step_names.len() {
                anyhow::bail!(
                    "Deadlock detected in stage '{}'! Check your 'needs' configuration.",
                    self.stage.name
                );
            }
        }

        for started in started_step_names.iter() {
            if !completed_step_names.contains(started) {
                step_reports.push(StepReport::failed(started, 0));
            }
        }

        for step in self.stage.steps.iter() {
            if !started_step_names.contains(&step.exploded_name) {
                step_reports.push(StepReport::skipped(&step.exploded_name));
            }
        }

        Ok(StageReport { step_reports })
    }

    fn can_start(&self, step: &Step, completed: &HashSet<String>) -> bool {
        step.needs.iter().all(|needed_base_name| {
            self.stage
                .steps
                .iter()
                .filter(|s| &s.name == needed_base_name)
                .all(|s| completed.contains(&s.exploded_name))
        })
    }
}
