use std::env;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use anyhow::Ok;
use chrono::Local;
use tokio::fs::create_dir_all;
use tokio_util::sync::CancellationToken;

use crate::{
    models::Pipeline,
    reporter::{ConsoleReporter, FileReporter},
    runner::PipelineRunner,
};

mod engine;
mod logger;
mod models;
mod reporter;
mod runner;
mod ui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cwd = env::current_dir()?;

    #[cfg(unix)]
    let user = {
        let meta = std::fs::metadata(&cwd)?;
        format!("{}:{}", meta.uid(), meta.gid())
    };
    #[cfg(not(unix))]
    let user = "0:0".to_string();

    let pipeline = Pipeline::new("ciroach.toml").await?;
    let runner = PipelineRunner::new(pipeline, user, cwd).await?;

    let token = CancellationToken::new();
    let signal_token = token.clone();

    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\n🛑 [SIGINT] Graceful shutdown initiated...");
            signal_token.cancel();
        }
    });

    let report = runner.run(token).await?;

    ConsoleReporter::report(&report);

    create_dir_all("logs").await?;
    let log_path = format!(
        "logs/build_{}.log",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    if let Err(err) = FileReporter::save(&report, &log_path).await {
        eprintln!("⚠️ Failed to save log file: {}", err);
    }

    if !report.is_success() {
        eprintln!("\n❌ Pipeline failed. See report for details.");
        std::process::exit(1);
    }

    println!("\n✨ Pipeline completed successfully!");
    Ok(())
}
