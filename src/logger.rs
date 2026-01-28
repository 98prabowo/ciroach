use std::collections::HashMap;

use tokio::{sync::mpsc, task::JoinHandle};

pub struct LogMessage {
    pub step_name: String,
    pub line: String,
    pub is_error: bool,
}

pub struct Logger {
    tx: mpsc::Sender<LogMessage>,
    handle: JoinHandle<HashMap<String, Vec<String>>>,
}

impl Logger {
    pub fn new(buffer: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<LogMessage>(buffer);
        let handle = tokio::spawn(async move {
            let mut store: HashMap<String, Vec<String>> = HashMap::new();
            while let Some(log) = rx.recv().await {
                let prefix = if log.is_error { "ERR" } else { "OUT" };
                let line = format!("[{}] [{}] {}", prefix, log.step_name, log.line.trim_end());
                store.entry(log.step_name).or_default().push(line);
            }
            store
        });

        Self { tx, handle }
    }

    pub fn tx(&self) -> mpsc::Sender<LogMessage> {
        self.tx.clone()
    }

    pub async fn finish(self) -> anyhow::Result<HashMap<String, Vec<String>>> {
        drop(self.tx); // Dropping the last TX allows RX to close
        self.handle
            .await
            .map_err(|err| anyhow::anyhow!("Log task failed: {err}"))
    }
}
