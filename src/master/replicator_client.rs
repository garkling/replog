use log;
use tokio::task::JoinSet;
use tonic::{Request, Response};
use tonic::transport::{Endpoint, Uri};

use replicator::Replica;
use replicator::replicator_client::{ReplicatorClient};

use replog::common::message::{Message, MessageLog};


pub mod replicator {
    tonic::include_proto!("replica");
}


impl From<&Message> for Replica {
    fn from(msg: &Message) -> Self {
        Self { content: msg.content.clone() }
    }
}

#[derive(Debug)]
pub struct ReplicatorMultiClient {

    endpoints: Vec<Uri>,
}

impl ReplicatorMultiClient {
    pub fn init(endpoints: impl Iterator<Item=String>) -> Self {
        let endpoints = endpoints               // todo: implement secondary server auto-detection
            .filter_map(|e| e.parse::<Uri>().ok())
            .collect();

        Self { endpoints }
    }

    pub async fn replicate(&self, message: Message) {
        let mut tasks = JoinSet::new();
        for end in self.endpoints.iter() {
            let Ok(mut client) = ReplicatorClient::connect(end.clone()).await else {
                log::error!("Cannot establish the connection with the given endpoint - {end}");
                continue
            };

            let host = end.host().unwrap_or("no-host").to_owned();
            let request_body = Replica::from(&message);
            let request = Request::new(request_body.clone());
            tasks.spawn(async move {
                log::info!("{host}: {:?} sent for replication", request_body);
                (client.replicate(request).await, host)
            });
        }

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((response, host)) => match response {
                    Ok(body) => log::info!("{host}: {:?} - replication status - {:?}", message, body.into_inner()),
                    Err(e) => log::error!("{host}: {:?} - replication failed - {e}", message)
                },
                Err(e) => {
                    log::debug!("Unable to join a thread - {e}");
                    log::error!("Message replication interrupted due to an internal error");
                }
            }
        }
    }
}