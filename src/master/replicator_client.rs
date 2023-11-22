use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::cmp::{max, min};
use std::time::Duration;

use log;
use uuid::Uuid;
use tokio::sync::Barrier;
use tokio::time;
use tonic::Request;
use tonic::transport::Uri;

use replicator::Replica;
use replicator::replicator_client::{ReplicatorClient};

use replog::common::message::Message;
use crate::RequestBody;


static GLOBAL_ORDERING: AtomicU32 = AtomicU32::new(1);


pub mod replicator {
    tonic::include_proto!("replica");
}


impl From<&Message> for Replica {
    fn from(msg: &Message) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            order: GLOBAL_ORDERING.fetch_add(1, Ordering::SeqCst),
            content: msg.content.clone()
        }
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

    pub async fn replicate(&self, message: Message, request: RequestBody) {

        let wc = max(
            min(self.endpoints.len() + 1, request.wc as usize),
            1
        );

        let mut request_body = Replica::from(&message);
        if let Some(order) = request.__ordering {
            request_body.order = order;
        }

        let wc_barrier = Arc::new(Barrier::new(wc));
        for end in self.endpoints.iter() {
            self.replicate_per_endpoint(
                request_body.clone(),
                end.clone(),
                wc_barrier.clone()
            ).await;

            if request.__duplicate {
                log::info!("Simulating message duplication");
                if let Ok(mut client) = ReplicatorClient::connect(end.clone()).await {
                    client.replicate(
                        Request::new(request_body.clone())
                    ).await;

                };
            }
        }

        match wc {
            1 => log::info!("master: non-blocking replication... WRITE CONCERN - {wc}"),
            _ => log::info!("master: blocking replication... WRITE CONCERN - {wc}")
        }
        wc_barrier.wait().await;
        log::info!("master: replication call completed")
    }

    async fn replicate_per_endpoint(&self, message: Replica, end: Uri, barrier: Arc<Barrier>) {

        let host = end.host().unwrap_or("secondary").to_owned();

        let Ok(mut client) = ReplicatorClient::connect(end.clone()).await else {
            log::error!("Cannot establish the connection with the given endpoint - {end}");
            return
        };

        let msg_content = message.content.clone();
        let request = Request::new(message);
        tokio::spawn(async move {
            log::info!("{host}: {:?} sent for replication", request);
            match client.replicate(request).await {
                Ok(body) => log::info!("{host}: Message {msg_content:?} - replication status - {:?}", body.into_inner()),
                Err(e) => log::error!("{host}: Message {msg_content:?} - replication failed - {e}")
            };

            barrier.wait().await;
        });
    }
}