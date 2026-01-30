use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineReport {
    pub stage_reports: Vec<StageReport>,
    pub logs: HashMap<String, Vec<String>>,
}

impl PipelineReport {
    pub fn is_success(&self) -> bool {
        self.stage_reports
            .iter()
            .flat_map(|step| &step.step_reports)
            .all(|step| step.status != StepStatus::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    pub step_reports: Vec<StepReport>,
}

impl StageReport {
    pub fn is_success(&self) -> bool {
        self.step_reports
            .iter()
            .all(|step| step.status != StepStatus::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    pub name: String,
    pub status: StepStatus,
    pub retries: u32,
    pub elapsed: u64,
}

impl StepReport {
    pub fn success(name: impl Into<String>, retries: u32, elapsed: u64) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Success,
            retries,
            elapsed,
        }
    }

    pub fn failed(name: impl Into<String>, retries: u32, elapsed: u64) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Failed,
            retries,
            elapsed,
        }
    }

    pub fn cancelled(name: impl Into<String>, retries: u32, elapsed: u64) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Cancelled,
            retries,
            elapsed,
        }
    }

    pub fn skipped(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Skipped,
            retries: 0,
            elapsed: 0,
        }
    }

    pub fn get_elasped_report(&self) -> String {
        if self.elapsed < 1000 {
            let elapsed = self.elapsed as f64 / 1000.0;
            format!("{elapsed:.2}")
        } else if self.elapsed == 0 {
            "0".to_string()
        } else {
            let elapsed = self.elapsed / 1000;
            format!("{elapsed}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Success,
    Failed,
    Cancelled,
    Skipped,
}
