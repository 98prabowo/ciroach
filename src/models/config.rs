use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub stages: Vec<Stage>,
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
