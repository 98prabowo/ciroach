use std::{path::Path, time::Duration};

use serde::Deserialize;
use tokio::fs::read_to_string;

use crate::models::RawPipeline;

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub stages: Vec<Stage>,
}

impl Pipeline {
    pub async fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config = read_to_string(path).await?;
        let raw: RawPipeline = toml::from_str(&config)?;
        let compiled = raw.compile()?;
        Ok(Self {
            stages: compiled.stages,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct Stage {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub name: String,
    pub exploded_name: String,
    pub image: String,
    pub memory: i64,
    pub needs: Vec<String>,
    pub env: Option<Vec<String>>,
    pub command: String,
    pub max_retries: u32,
    pub timeout: Duration,
}
