use std::{sync::Arc, time::Instant};

use anyhow::Ok;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    engine::DockerEngine,
    models::{LogMessage, Step, StepReport},
};

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
    ) -> StepReport {
        let timer = Instant::now();

        let step_name = self.step.exploded_name.clone();

        let outcome = tokio::select! {
            _ = token.cancelled() => {
                return StepReport::skipped(step_name);
            }
            res = self.execute(log_tx) => res
        };

        if outcome.is_ok() {
            StepReport::success(step_name, timer.elapsed().as_secs())
        } else {
            token.cancel();
            StepReport::failed(step_name, timer.elapsed().as_secs())
        }
    }

    async fn execute(&self, log_tx: mpsc::Sender<LogMessage>) -> anyhow::Result<()> {
        log_tx
            .send(LogMessage {
                step_name: self.step.exploded_name.clone(),
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
            .stream_logs(&id, &self.step.exploded_name, log_tx.clone())
            .await?;

        let state = self.engine.get_exit_state(&id).await?;

        if state.oom_killed == Some(true) {
            log_tx
                .send(LogMessage {
                    step_name: self.step.exploded_name.clone(),
                    line: "System ran out of memory".to_string(),
                    is_error: true,
                })
                .await
                .ok();

            anyhow::bail!(
                "System ran out of memory (Step: {})",
                self.step.exploded_name
            );
        }

        if state.exit_code != Some(0) {
            let code = state.exit_code.unwrap_or(-1);

            log_tx
                .send(LogMessage {
                    step_name: self.step.exploded_name.clone(),
                    line: format!("Process exited with code {code}"),
                    is_error: true,
                })
                .await
                .ok();

            anyhow::bail!(
                "Non-zero exit code {code} (Step: {})",
                self.step.exploded_name
            );
        }

        Ok(())
    }
}
