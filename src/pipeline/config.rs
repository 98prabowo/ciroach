use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PipelineConfig {
    pub pipeline: Pipeline,
}

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub name: String,
    pub image: String,
    pub memory: Option<String>,
    pub command: String,
    pub env: Option<Vec<String>>,
    pub parallel: Option<bool>,
}

impl Step {
    pub fn memory_limit(&self) -> anyhow::Result<i64> {
        const DEFAULT_MEMORY_LIMIT: i64 = 512 * 1024 * 1024;

        let mem = match &self.memory {
            Some(raw) => raw.to_lowercase(),
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
