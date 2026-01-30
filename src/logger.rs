use std::collections::HashMap;

use colored::Colorize;
use tokio::{sync::mpsc, task::JoinHandle};

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
                let line = log.terminal_format();
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

pub struct LogMessage {
    pub step_name: String,
    pub line: String,
    pub is_error: bool,
}

impl LogMessage {
    pub fn terminal_format(&self) -> String {
        let name = format!("[{}]", self.step_name).bold().cyan();
        let body = if self.is_error {
            self.line.trim_end().red()
        } else {
            self.line.trim_end().white()
        };
        format!("{name} {body}")
    }
}
