use std::collections::BTreeMap;

use regex::{Regex, escape};
use serde::Deserialize;

use crate::models::{Pipeline, Stage, Step};

const DEFAULT_MEMORY_LIMIT: i64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct RawPipeline {
    pub stages_order: Vec<String>,
    pub stages: BTreeMap<String, RawStage>,
}

impl RawPipeline {
    pub fn compile(self) -> anyhow::Result<Pipeline> {
        let mut final_stages = Vec::new();

        for stage_name in self.stages_order.iter() {
            let Some(raw_stage) = self.stages.get(stage_name) else {
                println!(
                    "⚠️ Stage '{}' declared in order but missing definition. Skipping.",
                    stage_name
                );
                continue;
            };

            if raw_stage.steps.is_empty() {
                println!("⚠️ Stage '{}' is empty. Skipping.", stage_name);
                continue;
            }

            let mut resolved_steps = Vec::new();

            for (step_id, step_cfg) in raw_stage.steps.iter() {
                if let Some(matrix) = step_cfg.matrix.as_ref() {
                    let pattern = format!(r"\$\{{\{{\s*{}\s*\}}\}}", escape(&matrix.variable));
                    let regex = Regex::new(&pattern)?;

                    for val in matrix.values.iter() {
                        resolved_steps.push(Step {
                            name: step_id.to_string(),
                            exploded_name: format!("{}-{}", step_id, val),
                            image: regex.replace_all(&step_cfg.image, val).to_string(),
                            memory: step_cfg.memory_limit()?,
                            needs: step_cfg.needs.clone().unwrap_or_default(),
                            env: step_cfg.env.clone(),
                            command: regex.replace_all(&step_cfg.command, val).to_string(),
                        });
                    }
                } else {
                    resolved_steps.push(Step {
                        name: step_id.clone(),
                        exploded_name: step_id.clone(),
                        image: step_cfg.image.clone(),
                        memory: step_cfg.memory_limit()?,
                        needs: step_cfg.needs.clone().unwrap_or_default(),
                        env: step_cfg.env.clone(),
                        command: step_cfg.command.clone(),
                    });
                }
            }

            final_stages.push(Stage {
                name: stage_name.clone(),
                steps: resolved_steps,
            });
        }

        if final_stages.is_empty() {
            anyhow::bail!("No valid stages or steps found to execute.");
        }

        Ok(Pipeline {
            stages: final_stages,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct RawStage {
    pub steps: BTreeMap<String, RawStep>,
}

#[derive(Debug, Deserialize)]
pub struct RawStep {
    pub image: String,
    pub command: String,
    pub memory: Option<String>,
    pub needs: Option<Vec<String>>,
    pub env: Option<Vec<String>>,
    pub matrix: Option<MatrixConfig>,
}

impl RawStep {
    pub fn memory_limit(&self) -> anyhow::Result<i64> {
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

#[derive(Debug, Deserialize)]
pub struct MatrixConfig {
    pub variable: String,
    pub values: Vec<String>,
}
