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
    pub elapsed: u64,
}

impl StepReport {
    pub fn success(name: impl Into<String>, elapsed: u64) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Success,
            elapsed,
        }
    }

    pub fn failed(name: impl Into<String>, elapsed: u64) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Failed,
            elapsed,
        }
    }

    pub fn skipped(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: StepStatus::Skipped,
            elapsed: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Success,
    Failed,
    Skipped,
}

pub struct LogMessage {
    pub step_name: String,
    pub line: String,
    pub is_error: bool,
}
