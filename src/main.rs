use std::env;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Ok;
use bollard::Docker;
use bollard::container::LogOutput;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, LogsOptionsBuilder,
    RemoveContainerOptionsBuilder,
};
use bollard::secret::{ContainerState, HostConfig};
use chrono::Local;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::fs::{File, create_dir_all, read_to_string};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = Arc::new(DockerEngine::new()?);
    create_dir_all("logs").await?;

    let content = read_to_string("examples/pipeline.toml").await?;
    let config: PipelineConfig = toml::from_str(&content)?;

    let log_name = format!(
        "logs/build_{}.log",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let mut log_file = File::create(&log_name).await?;

    let mut handles = vec![];
    let (tx, mut rx) = mpsc::channel(100);

    let cancel_token = CancellationToken::new();

    for step in config.pipeline.steps {
        let engine = Arc::clone(&engine);
        let log_tx = tx.clone();
        let cwd = env::current_dir()?;
        let cwd_str = cwd.to_string_lossy().to_string();
        let token = cancel_token.clone();

        #[cfg(unix)]
        let user_mapping = {
            let meta = std::fs::metadata(&cwd)?;
            format!("{}:{}", meta.uid(), meta.gid())
        };
        #[cfg(not(unix))]
        let user_mapping = "0:0".to_string();

        let handle = tokio::spawn(async move {
            if token.is_cancelled() {
                return StepResult {
                    name: step.name,
                    status: StepStatus::Skipped,
                    elapsed: 0,
                };
            }
            execute_step(step, engine, log_tx, &cwd_str, user_mapping, token).await
        });

        handles.push(handle);
    }

    drop(tx);

    while let Some(log) = rx.recv().await {
        handle_line(
            &log.step_name,
            &log.line,
            &mut log_file,
            if log.is_error { "ERR" } else { "OUT" },
        )
        .await?;
    }

    let mut final_results = vec![];
    let mut all_success = true;

    for handle in handles {
        let result = handle.await?;
        if matches!(result.status, StepStatus::Failed) {
            all_success = false;
        }
        final_results.push(result);
    }

    println!("\n--- 🪳 Pipeline Summary ---");

    for res in final_results {
        let (icon, msg) = match res.status {
            StepStatus::Success => ("✅", format!("finished after {}s", res.elapsed)),
            StepStatus::Failed => ("❌", "failed".to_string()),
            StepStatus::Skipped => ("⏭️", "skipped".to_string()),
        };
        println!("{icon} {}: {}", res.name, msg);
    }

    if !all_success {
        anyhow::bail!("Pipeline failed.");
    }

    Ok(())
}

async fn handle_line(step: &str, msg: &str, file: &mut File, mode: &str) -> anyhow::Result<()> {
    let timestamp = Local::now().format("%H:%M:%S");
    let line = format!("[{timestamp}] [{step}] [{mode}] {}\n", msg.trim_end());
    print!("{line}");
    file.write_all(line.as_bytes()).await?;
    file.flush().await?;
    Ok(())
}

async fn execute_step(
    step: Step,
    engine: Arc<DockerEngine>,
    log_tx: mpsc::Sender<LogMessage>,
    cwd: &str,
    user_mapping: String,
    token: CancellationToken,
) -> StepResult {
    let step_start = Instant::now();

    let outcome = tokio::select! {
        _ = token.cancelled() => {
            return StepResult {
                name: step.name,
                status: StepStatus::Skipped,
                elapsed: 0,
            };
        }
        res = async {
            engine.pull_image(&step).await?;
            let id = engine.run_container(&step, cwd, user_mapping).await?;
            engine.stream_logs(&id, &step.name, log_tx).await?;
            let state = engine.get_exit_state(&id).await?;

            if state.oom_killed == Some(true) {
                token.cancel();
                anyhow::bail!("System ran out of memory (Step: {})", step.name);
            }

            if state.exit_code != Some(0) {
                token.cancel();
                anyhow::bail!("Non-zero exit code (Step: {})", step.name);
            }

            Ok(())
        } => res
    };

    let status = if outcome.is_ok() {
        StepStatus::Success
    } else {
        StepStatus::Failed
    };

    StepResult {
        name: step.name.clone(),
        status,
        elapsed: step_start.elapsed().as_secs(),
    }
}

struct DockerEngine {
    client: Docker,
}

impl DockerEngine {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: Docker::connect_with_local_defaults()?,
        })
    }

    pub async fn pull_image(&self, step: &Step) -> anyhow::Result<()> {
        let image_options = CreateImageOptionsBuilder::new()
            .from_image(&step.image)
            .build();

        let mut pull_stream = self.client.create_image(Some(image_options), None, None);

        while let Some(pull_result) = pull_stream.next().await {
            pull_result?;
        }

        Ok(())
    }

    pub async fn run_container(
        &self,
        step: &Step,
        cwd: &str,
        user_mapping: String,
    ) -> anyhow::Result<String> {
        let container_name = format!("ciroach-{}", step.name.replace(" ", "-"));
        let container_options = CreateContainerOptionsBuilder::new()
            .name(&container_name)
            .build();

        let cmd = vec!["sh".to_string(), "-c".to_string(), step.command.clone()];

        let memory_limit = Self::parse_memory_limit(&step.memory)?;

        let host_config = HostConfig {
            binds: Some(vec![format!("{cwd}:/workspace")]),
            memory: Some(memory_limit),
            memory_swap: Some(memory_limit),
            ..Default::default()
        };

        let container_config = ContainerCreateBody {
            user: Some(user_mapping),
            env: step.env.clone(),
            cmd: Some(cmd),
            image: Some(step.image.clone()),
            working_dir: Some("/workspace".to_string()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container = self
            .client
            .create_container(Some(container_options), container_config)
            .await?;

        self.client.start_container(&container.id, None).await?;

        Ok(container.id)
    }

    pub async fn stream_logs(
        &self,
        id: &str,
        step_name: &str,
        log_tx: mpsc::Sender<LogMessage>,
    ) -> anyhow::Result<()> {
        let logs_options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .follow(true)
            .build();

        let mut stream = self.client.logs(id, Some(logs_options));

        while let Some(log) = stream.next().await {
            let (line, is_error) = match log? {
                LogOutput::StdOut { message } => {
                    (String::from_utf8_lossy(&message).to_string(), false)
                }
                LogOutput::StdErr { message } => {
                    (String::from_utf8_lossy(&message).to_string(), true)
                }
                _ => continue,
            };

            log_tx
                .send(LogMessage {
                    step_name: step_name.to_string(),
                    line,
                    is_error,
                })
                .await
                .ok();
        }

        Ok(())
    }

    pub async fn get_exit_state(&self, id: &str) -> anyhow::Result<ContainerState> {
        let inspect = self.client.inspect_container(id, None).await?;
        let state = inspect.state.unwrap_or_default();

        let remove_options = RemoveContainerOptionsBuilder::new().force(true).build();

        self.client
            .remove_container(id, Some(remove_options))
            .await?;

        Ok(state)
    }

    fn parse_memory_limit(mem_str: &Option<String>) -> anyhow::Result<i64> {
        const DEFAULT_MEMORY_LIMIT: i64 = 512 * 1024 * 1024;

        let mem = match mem_str {
            Some(s) => s.to_lowercase(),
            None => return Ok(DEFAULT_MEMORY_LIMIT),
        };

        let (digits, multiplier) = if mem.ends_with("gb") {
            (mem.replace("gb", ""), 1024 * 1024 * 1024)
        } else if mem.ends_with("mb") {
            (mem.replace("mb", ""), 1024 * 1024)
        } else if mem.ends_with("kb") {
            (mem.replace("kb", ""), 1024)
        } else {
            (mem, 1)
        };

        let value = digits.trim().parse::<i64>().map_err(|_| {
            anyhow::anyhow!("Invalid memory format: '{}'. Use '512mb' or '1gb'", digits)
        })?;

        Ok(value * multiplier)
    }
}

#[derive(Debug, Deserialize)]
struct PipelineConfig {
    pipeline: Pipeline,
}

#[derive(Debug, Deserialize)]
struct Pipeline {
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    name: String,
    image: String,
    memory: Option<String>,
    command: String,
    env: Option<Vec<String>>,
}

struct LogMessage {
    step_name: String,
    line: String,
    is_error: bool,
}

struct StepResult {
    name: String,
    status: StepStatus,
    elapsed: u64,
}

#[derive(Debug, Clone, Copy)]
enum StepStatus {
    Success,
    Failed,
    Skipped,
}
