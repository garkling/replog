use std::sync::Arc;
use std::cmp::{max, min};

use log;
use tokio::sync::Barrier;
use tonic::Request;
use tonic::transport::Uri;

use replicator::Replica;
use replicator::replicator_client::{ReplicatorClient};

use replog::common::message::Message;


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

    pub async fn replicate(&self, message: Message, wc: u8) {

        let wc = max(
            min(self.endpoints.len() + 1, wc as usize),
            1
        );

        let request_body = Replica::from(&message);
        let wc_barrier = Arc::new(Barrier::new(wc));
        for end in self.endpoints.iter() {
            let Ok(mut client) = ReplicatorClient::connect(end.clone()).await else {
                log::error!("Cannot establish the connection with the given endpoint - {end}");
                continue
            };

            let host = end.host().unwrap_or("no-host").to_owned();
            let request = Request::new(request_body.clone());

            let cloned_message = message.clone();
            let thread_barrier = wc_barrier.clone();
            tokio::spawn(async move {
                log::info!("{host}: {:?} sent for replication", request);
                match client.replicate(request).await {
                    Ok(body) => log::info!("{host}: {cloned_message:?} - replication status - {:?}", body.into_inner()),
                    Err(e) => log::error!("{host}: {cloned_message:?} - replication failed - {e}")
                };

                thread_barrier.wait().await;
            });
        }

        match wc {
            1 => log::debug!("master: non-blocking replication... WRITE CONCERN - {wc}"),
            _ => log::debug!("master: blocking replication... WRITE CONCERN - {wc}")
        }
        wc_barrier.wait().await;
        log::debug!("master: replication call completed")
    }
}