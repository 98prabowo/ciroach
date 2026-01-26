use std::env;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

use anyhow::Ok;
use bollard::container::LogOutput;
use bollard::Docker;
use bollard::query_parameters::{CreateContainerOptionsBuilder, CreateImageOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder};
use bollard::models::ContainerCreateBody;
use bollard::secret::HostConfig;
use chrono::Local;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::fs::{File, create_dir_all, read_to_string};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let start_time = Instant::now();

    let docker = Docker::connect_with_local_defaults()?;
    create_dir_all("logs").await?;

    let content = read_to_string("pipeline.toml").await
        .expect("Could not find pipeline.toml");
    let config: PipelineConfig = toml::from_str(&content)?;

    let log_name = format!("logs/build_{}.log", Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let mut log_file = File::create(&log_name).await?;

    let mut pipeline_success = true;

    for step in config.pipeline.steps {
        println!("🪳 Step: {} (Image: {})", step.name, step.image);
        let step_start = Instant::now();

        if !pipeline_success && step.name != "Cleanup" {
            println!("  ⏭️ Skipping: {}", step.name);
            continue;
        }

        let mut pull_stream = docker.create_image(
            Some(
                CreateImageOptionsBuilder::new()
                    .from_image(&step.image)
                    .build()
            ), 
            None,
            None
        );

        while let Some(pull_result) = pull_stream.next().await {
            pull_result?;
        }

        let container_name = format!("ciroach-{}", step.name.replace(" ", "-"));
        let options = CreateContainerOptionsBuilder::new()
            .name(&container_name)
            .build();

        let cwd = env::current_dir()?;
        let cwd_str = cwd.to_string_lossy().to_string();

        #[cfg(unix)]
        let user_mapping = {
            let meta = std::fs::metadata(&cwd)?;
            format!("{}:{}", meta.uid(), meta.gid())
        };
        #[cfg(not(unix))]
        let user_mapping = "0:0".to_string();

        let cmd = vec![
            "sh".to_string(), 
            "-c".to_string(), 
            step.command.clone()
        ];

        let memory_limit = parse_memory_limit(&step.memory)?;

        let host_config = HostConfig {
            binds: Some(vec![format!("{cwd_str}:/workspace")]),
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

        let container = docker.create_container(Some(options), container_config).await?;
        docker.start_container(&container.id, None).await?;

        let logs_options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .follow(true)
            .build();

        let mut stream = docker.logs(&container.id, Some(logs_options));

        while let Some(log) = stream.next().await {
            match log? {
                LogOutput::StdOut { message } => {
                    handle_line(&step.name, &String::from_utf8_lossy(&message), &mut log_file, "OUT").await?;
                }
                LogOutput::StdErr { message } => {
                    handle_line(&step.name, &String::from_utf8_lossy(&message), &mut log_file, "ERR").await?;
                }
                _ => {}
            }
        }

        let inspect = docker.inspect_container(&container.id, None).await?;
        let state = inspect.state.unwrap_or_default();

        if let Some(true) = state.oom_killed {
            println!("  🚨 ERROR: Step '{}' was killed because it ran out of memory!", step.name);
            pipeline_success = false;
        }

        let remove_options = RemoveContainerOptionsBuilder::new()
            .force(true)
            .build();

        docker.remove_container(&container.id, Some(remove_options)).await?;

        // let exit_code = inspect.state.and_then(|s| s.exit_code).unwrap_or(0);
        let exit_code = state.exit_code.unwrap_or_default();
        if exit_code != 0 {
            println!("  ❌ Step '{}' failed (Code {}) after {}s", step.name, exit_code, start_time.elapsed().as_secs());
            pipeline_success = false;
            continue;
        }

        println!("  ✅ Completed in {}s", step_start.elapsed().as_secs());
    }

    if pipeline_success {
        println!("\n🚀 Pipeline Finished Successfully in {}s!", start_time.elapsed().as_secs());
        Ok(())
    } else {
        println!("\n⚠️ Pipeline FAILED. Check logs for details.");
        anyhow::bail!("Pipeline execution failed")
    }
}

async fn handle_line(step: &str, msg: &str, file: &mut File, mode: &str) -> anyhow::Result<()> {
    let timestamp = Local::now().format("%H:%M:%S");
    let line = format!("[{timestamp}] [{step}] [{mode}] {}\n", msg.trim_end());
    print!("{line}");
    file.write_all(line.as_bytes()).await?;
    file.flush().await?;
    Ok(())
}

fn parse_memory_limit(mem_str: &Option<String>) -> anyhow::Result<i64> {
    const DEFAULT_MEMORY_LIMIT: i64 = 512 * 1024 * 1024;

    let mem = match mem_str {
        Some(s) => s.to_lowercase(),
        None => return Ok(DEFAULT_MEMORY_LIMIT),
    };

    let (digits, multiplier)  = if mem.ends_with("gb") {
        (mem.replace("gb", ""), 1024 * 1024 * 1024)
    } else if mem.ends_with("mb") {
        (mem.replace("mb", ""), 1024 * 1024)
    } else if mem.ends_with("kb") {
        (mem.replace("kb", ""), 1024)
    } else {
        (mem, 1)
    };

    let value = digits
        .trim()
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("Invalid memory format: '{}'. Use '512mb' or '1gb'", digits))?;

    Ok(value * multiplier)
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
