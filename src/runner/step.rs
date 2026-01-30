use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Ok;
use tokio::{
    sync::{Mutex, mpsc},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    engine::DockerEngine,
    logger::LogMessage,
    models::{Step, StepReport},
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
        let mut attempts = 0;
        let max_retries = self.step.max_retries;
        let step_name = &self.step.exploded_name;

        loop {
            match self.execute_attempt(&log_tx, &token).await {
                std::result::Result::Ok(_) => {
                    return StepReport::success(
                        step_name,
                        attempts,
                        timer.elapsed().as_millis() as u64,
                    );
                }
                std::result::Result::Err(err)
                    if token.is_cancelled() && err.to_string() == "Cancelled" =>
                {
                    return StepReport::cancelled(
                        step_name,
                        attempts,
                        timer.elapsed().as_millis() as u64,
                    );
                }
                std::result::Result::Err(err) => {
                    if attempts < self.step.max_retries && !token.is_cancelled() {
                        attempts += 1;

                        self.log_retry(&log_tx, attempts, max_retries, &err).await;

                        let throttle_secs = 2u64.pow(attempts);
                        let throttle_duration = Duration::from_secs(throttle_secs);

                        tokio::select! {
                            _ = sleep(throttle_duration) => {
                                continue;
                            }
                            _ = token.cancelled() => {
                                return StepReport::cancelled(step_name, attempts, timer.elapsed().as_millis() as u64);
                            }
                        }
                    }

                    token.cancel();
                    return StepReport::failed(
                        step_name,
                        attempts,
                        timer.elapsed().as_millis() as u64,
                    );
                }
            }
        }
    }

    async fn execute_attempt(
        &self,
        log_tx: &mpsc::Sender<LogMessage>,
        token: &CancellationToken,
    ) -> anyhow::Result<()> {
        let container_id = Arc::new(Mutex::new(None));
        let exec_id = Arc::clone(&container_id);

        let exec_fut = self.execute(log_tx, exec_id, token);
        let timeout_fut = timeout(self.step.timeout, exec_fut);

        tokio::select! {
            _ = token.cancelled() => {
                self.cleanup_container(&container_id).await;
                Err(anyhow::anyhow!("Cancelled"))
            }
            res = timeout_fut => match res {
                std::result::Result::Ok(inner) => inner,
                std::result::Result::Err(_) => {
                    self.log_timeout(log_tx, self.step.timeout).await;
                    self.cleanup_container(&container_id).await;
                    Err(anyhow::anyhow!("Timeout"))
                }
            }
        }
    }

    async fn execute(
        &self,
        log_tx: &mpsc::Sender<LogMessage>,
        id_tracker: Arc<Mutex<Option<String>>>,
        token: &CancellationToken,
    ) -> anyhow::Result<()> {
        let id = self
            .engine
            .run_container(&self.step, &self.cwd, &self.user)
            .await?;

        Self::save_running_container_id(id_tracker, &id).await;

        self.engine
            .stream_logs(&id, &self.step.exploded_name, log_tx, token)
            .await?;

        let state = self.engine.get_exit_state(&id).await?;

        if state.oom_killed == Some(true) {
            self.log_oom(log_tx).await;
            anyhow::bail!(
                "System ran out of memory (Step: {})",
                self.step.exploded_name
            );
        }

        if state.exit_code != Some(0) {
            let code = state.exit_code.unwrap_or(-1);
            self.log_bad_exit_code(log_tx, code).await;
            anyhow::bail!(
                "Non-zero exit code {code} (Step: {})",
                self.step.exploded_name
            );
        }

        Ok(())
    }

    async fn cleanup_container(&self, id_mutex: &Arc<Mutex<Option<String>>>) {
        let mut guard = id_mutex.lock().await;
        if let Some(id) = guard.take() {
            self.engine.force_remove_container(&id).await.ok();
        }
    }

    async fn save_running_container_id(
        id_tracker: Arc<Mutex<Option<String>>>,
        id: impl Into<String>,
    ) {
        let mut guard = id_tracker.lock().await;
        *guard = Some(id.into());
    }
}

impl StepRunner {
    async fn log_timeout(&self, tx: &mpsc::Sender<LogMessage>, timeout: Duration) {
        tx.send(LogMessage {
            step_name: self.step.exploded_name.clone(),
            line: format!("⏳ Step timed out after {:?}", timeout),
            is_error: true,
        })
        .await
        .ok();
    }

    async fn log_retry(
        &self,
        tx: &mpsc::Sender<LogMessage>,
        attempts: u32,
        max_retries: u32,
        err: &anyhow::Error,
    ) {
        tx.send(LogMessage {
            step_name: self.step.exploded_name.clone(),
            line: format!(
                "🔄 Retrying step ({}/{}) - Error: {}",
                attempts, max_retries, err
            ),
            is_error: true,
        })
        .await
        .ok();
    }

    async fn log_oom(&self, tx: &mpsc::Sender<LogMessage>) {
        tx.send(LogMessage {
            step_name: self.step.exploded_name.clone(),
            line: "System ran out of memory".to_string(),
            is_error: true,
        })
        .await
        .ok();
    }

    async fn log_bad_exit_code(&self, tx: &mpsc::Sender<LogMessage>, code: i64) {
        tx.send(LogMessage {
            step_name: self.step.exploded_name.clone(),
            line: format!("Process exited with code {code}"),
            is_error: true,
        })
        .await
        .ok();
    }
}
