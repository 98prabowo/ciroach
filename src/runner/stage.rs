use std::{collections::HashSet, sync::Arc};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    engine::DockerEngine,
    logger::LogMessage,
    models::{Stage, StageReport, Step, StepReport},
    runner::StepRunner,
};

#[derive(Debug, Default)]
struct StageState {
    pub started: HashSet<String>,
    pub completed: HashSet<String>,
    pub reports: Vec<StepReport>,
}

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
        let mut state = StageState::default();
        let (status_tx, mut status_rx) = mpsc::channel::<StepReport>(100);
        let total_steps = self.stage.steps.len();

        loop {
            if !token.is_cancelled() {
                self.dispatch_ready_steps(&mut state, &log_tx, &status_tx, &token);
            }

            if state.started.len() == state.completed.len() {
                // If the token was cancelled, and we have received reports for everything we started,
                // we can safely stop the loop even if some steps in the stage never ran.
                if token.is_cancelled() || state.started.len() == total_steps {
                    break;
                }

                // If we aren't cancelled, but nothing is running and we aren't finished, it's a deadlock.
                if !token.is_cancelled() {
                    anyhow::bail!(
                        "Deadlock detected in stage '{}'! Check your 'needs' configuration.",
                        self.stage.name
                    );
                }
            }

            if let Some(rep) = status_rx.recv().await {
                state.completed.insert(rep.name.clone());
                state.reports.push(rep);
            } else {
                break;
            }
        }

        Ok(self.finalize_report(state))
    }

    fn dispatch_ready_steps(
        &self,
        state: &mut StageState,
        log_tx: &mpsc::Sender<LogMessage>,
        status_tx: &mpsc::Sender<StepReport>,
        token: &CancellationToken,
    ) {
        for step in self.stage.steps.iter() {
            if state.started.contains(&step.exploded_name) {
                continue;
            }

            if self.can_start(step, &state.completed) {
                state.started.insert(step.exploded_name.clone());

                let runner =
                    StepRunner::new(step.clone(), self.engine.clone(), &self.cwd, &self.user);

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

    fn can_start(&self, step: &Step, completed: &HashSet<String>) -> bool {
        step.needs.iter().all(|needed_base_name| {
            self.stage
                .steps
                .iter()
                .filter(|s| &s.name == needed_base_name)
                .all(|s| completed.contains(&s.exploded_name))
        })
    }

    fn finalize_report(&self, mut state: StageState) -> StageReport {
        let finished_names: HashSet<String> = state
            .reports
            .iter()
            .map(|report| report.name.clone())
            .collect();

        for step in self.stage.steps.iter() {
            if !finished_names.contains(&step.exploded_name) {
                if state.started.contains(&step.exploded_name) {
                    state.reports.push(StepReport::failed(&step.exploded_name, 0));
                } else {
                    state.reports.push(StepReport::skipped(&step.exploded_name));
                }
            }
        }

        StageReport {
            step_reports: state.reports,
        }
    }
}
