use std::env;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use chrono::Local;
use tokio::fs::{File, create_dir_all};

use crate::pipeline::PipelineRunner;

mod engine;
mod pipeline;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    create_dir_all("logs").await?;
    let pipeline_path = "examples/pipeline.toml";

    let log_name = format!(
        "logs/build_{}.log",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let log_file = File::create(&log_name).await?;

    let cwd = env::current_dir()?;

    #[cfg(unix)]
    let user = {
        let meta = std::fs::metadata(&cwd)?;
        format!("{}:{}", meta.uid(), meta.gid())
    };
    #[cfg(not(unix))]
    let user = "0:0".to_string();

    let runner = PipelineRunner::new(pipeline_path, user, cwd).await?;
    runner.execute(log_file).await
}
