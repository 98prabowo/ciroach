use std::{sync::Arc, time::Instant};

use anyhow::Ok;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{engine::DockerEngine, pipeline::{LogMessage, Step, StepResult}};

pub struct StepRunner {
    step: Step,
    engine: Arc<DockerEngine>,
    cwd: String,
    user: String,
}

impl StepRunner {
    pub fn new(
        step: Step,
        engine: Arc<DockerEngine>,
        cwd: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            step,
            engine,
            cwd: cwd.into(),
            user: user.into(),
        }
    }

    pub async fn run(
        self,
        log_tx: mpsc::Sender<LogMessage>,
        token: CancellationToken,
    ) -> StepResult {
        let timer = Instant::now();

        let outcome = tokio::select! {
            _ = token.cancelled() => {
                return StepResult::skipped(self.step.name.clone());
            }
            res = self.execute(log_tx) => res
        };

        if outcome.is_err() {
            token.cancel();
            StepResult::failed(self.step.name.clone(), timer.elapsed().as_secs())
        } else {
            StepResult::success(self.step.name.clone(), timer.elapsed().as_secs())
        }
    }

    async fn execute(&self, log_tx: mpsc::Sender<LogMessage>) -> anyhow::Result<()> {
        log_tx
            .send(LogMessage {
                step_name: self.step.name.clone(),
                line: format!("Preparing image: {}", self.step.image),
                is_error: false,
            })
            .await
            .ok();

        let id = self
            .engine
            .run_container(&self.step, self.cwd.clone(), self.user.clone())
            .await?;

        self.engine
            .stream_logs(&id, &self.step.name, log_tx)
            .await?;

        let state = self.engine.get_exit_state(&id).await?;

        if state.oom_killed == Some(true) {
            anyhow::bail!("System ran out of memory (Step: {})", self.step.name);
        }

        if state.exit_code != Some(0) {
            anyhow::bail!("Non-zero exit code (Step: {})", self.step.name);
        }

        Ok(())
    }
}
