use anyhow::Ok;
use tokio::{fs::File, io::AsyncWriteExt};

use crate::models::PipelineReport;

pub struct FileReporter;

impl FileReporter {
    pub async fn save(report: &PipelineReport, path: &str) -> anyhow::Result<()> {
        let mut file = File::create(path).await?;
        let mut buffer = String::new();

        buffer.push_str("--- Pipeline Report ---\n\n");

        for stage in report.stage_reports.iter() {
            for step in stage.step_reports.iter() {
                let status_str = format!("{:?}", step.status);
                buffer.push_str(&format!(
                    "Step: {} | Status {} | Duration {}s\n",
                    step.name,
                    status_str,
                    step.get_elasped_report(),
                ));
            }
        }

        file.write_all(buffer.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }
}
