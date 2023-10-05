use std::sync::Arc;

use log;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub content: String
}

pub struct MessageLog {
    messages: Arc<Mutex<Vec<Message>>>
}

impl MessageLog {

    pub fn new() -> Self {
        let messages = Arc::new(Mutex::new(vec![]));

        Self { messages }
    }

    pub async fn add(&self, msg: Message) {
        let mut messages = self.messages.lock().await;

        let content = msg.content.clone();

        messages.push(msg);
        log::info!("Message `{}` appended", content)
    }

    pub async fn get_all(&self) -> Vec<Message> {
        let messages = self.messages.lock().await;

        messages.clone()
    }

}

impl From<&MessageLog> for MessageLog {

    fn from(log: &MessageLog) -> Self {
        Self {
            messages: log.messages.clone()
        }
    }
}