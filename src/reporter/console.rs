use crate::models::{PipelineReport, StepStatus};

pub struct ConsoleReporter;

impl ConsoleReporter {
    pub fn report(report: &PipelineReport) {
        println!("\n--- 📖 Pipeline Execution Logs ---");

        for (step_name, lines) in report.logs.iter() {
            println!("\n=== {} ===", step_name.to_uppercase());
            for line in lines {
                println!("{line}");
            }
        }

        println!("\n--- 🪳 Summary ---");

        for (index, stage) in report.stage_reports.iter().enumerate() {
            println!("Stage {}:", index + 1);
            for step in stage.step_reports.iter() {
                let icon = match step.status {
                    StepStatus::Success => "✅",
                    StepStatus::Failed => "❌",
                    StepStatus::Skipped => "⏭️",
                };
                println!("  {} {}: {}s", icon, step.name, step.elapsed);
            }
        }
    }
}
