use colored::Colorize;

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

        println!(
            "\n{}",
            "--- 🪳 Final Pipeline Report ---\n".bold().underline()
        );

        println!(
            "{:<4} {:<30} {:<12} {:<10} {:<12}",
            "No".bold(),
            "Step Name".bold(),
            "Status".bold(),
            "Retries".bold(),
            "Duration".bold(),
        );

        println!("{}", "-".repeat(70).dimmed());

        let mut report_index = 1;

        for stage in report.stage_reports.iter() {
            for step in stage.step_reports.iter() {
                let status = match step.status {
                    StepStatus::Success => "PASS".green().bold(),
                    StepStatus::Failed => "FAIL".red().bold(),
                    StepStatus::Cancelled => "STOP".yellow().bold(),
                    StepStatus::Skipped => "SKIP".white().dimmed(),
                };

                println!(
                    "{:<4} {:<30} {:<12} {:<10} {:<12}",
                    report_index,
                    step.name.cyan(),
                    status,
                    step.retries,
                    format!("{}s", step.get_elasped_report()),
                );

                report_index += 1;
            }
        }

        println!("{}", "-".repeat(70).dimmed());
    }
}
