pub struct LogMessage {
    pub step_name: String,
    pub line: String,
    pub is_error: bool,
}

pub struct StepResult {
    pub name: String,
    pub status: StepStatus,
    pub elapsed: u64,
}

impl StepResult {
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
