use bollard::{
    Docker,
    container::LogOutput,
    query_parameters::{
        CreateContainerOptionsBuilder, CreateImageOptionsBuilder, LogsOptionsBuilder,
        RemoveContainerOptionsBuilder,
    },
    secret::{ContainerCreateBody, ContainerState, HostConfig},
};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::{logger::LogMessage, models::Step};

pub struct DockerEngine {
    client: Docker,
}

impl DockerEngine {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: Docker::connect_with_local_defaults()?,
        })
    }

    pub async fn pull_image(&self, image: &str) -> anyhow::Result<()> {
        let image_options = CreateImageOptionsBuilder::new().from_image(image).build();

        let mut pull_stream = self.client.create_image(Some(image_options), None, None);

        while let Some(pull_result) = pull_stream.next().await {
            pull_result?;
        }

        Ok(())
    }

    pub async fn run_container(
        &self,
        step: &Step,
        cwd: impl Into<String>,
        user: impl Into<String>,
    ) -> anyhow::Result<String> {
        let container_name = format!("ciroach-{}", step.exploded_name.replace(" ", "-"));

        self.force_remove_container(&container_name).await.ok();

        let container_options = CreateContainerOptionsBuilder::new()
            .name(&container_name)
            .build();

        let cmd = vec!["sh".to_string(), "-c".to_string(), step.command.clone()];

        let host_config = HostConfig {
            binds: Some(vec![format!("{}:/workspace", cwd.into())]),
            memory: Some(step.memory),
            memory_swap: Some(step.memory),
            ..Default::default()
        };

        let container_config = ContainerCreateBody {
            user: Some(user.into()),
            env: step.env.clone(),
            cmd: Some(cmd),
            image: Some(step.image.clone()),
            working_dir: Some("/workspace".to_string()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container = self
            .client
            .create_container(Some(container_options), container_config)
            .await?;

        self.client.start_container(&container.id, None).await?;

        Ok(container.id)
    }

    pub async fn stream_logs(
        &self,
        id: &str,
        step_name: &str,
        log_tx: &mpsc::Sender<LogMessage>,
    ) -> anyhow::Result<()> {
        let logs_options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .follow(true)
            .build();

        let mut stream = self.client.logs(id, Some(logs_options));

        while let Some(log) = stream.next().await {
            let (line, is_error) = match log? {
                LogOutput::StdOut { message } => {
                    (String::from_utf8_lossy(&message).to_string(), false)
                }
                LogOutput::StdErr { message } => {
                    (String::from_utf8_lossy(&message).to_string(), true)
                }
                _ => continue,
            };

            log_tx
                .send(LogMessage {
                    step_name: step_name.to_string(),
                    line,
                    is_error,
                })
                .await
                .ok();
        }

        Ok(())
    }

    pub async fn get_exit_state(&self, id: &str) -> anyhow::Result<ContainerState> {
        let inspect = self.client.inspect_container(id, None).await?;
        let state = inspect.state.unwrap_or_default();
        self.force_remove_container(id).await.ok();
        Ok(state)
    }

    pub async fn force_remove_container(&self, name: &str) -> anyhow::Result<()> {
        let remove_options = RemoveContainerOptionsBuilder::new().force(true).build();

        self.client
            .remove_container(name, Some(remove_options))
            .await?;

        Ok(())
    }
}
