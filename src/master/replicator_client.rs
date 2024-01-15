use std::cmp::{max, min};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log;
use uuid::Uuid;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use tokio::sync::{Barrier, Mutex};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tonic::Request;
use tonic::transport::Channel;
use async_channel as ac;

use replicator::Replica;
use replicator::replicator_client::ReplicatorClient;
use replog::WRITE_QUORUM;
use replog::common::retry::Attempts;
use crate::{RequestBody, Message};

pub mod replicator {
    tonic::include_proto!("replica");
}

static GLOBAL_ORDERING: AtomicU32 = AtomicU32::new(1);

impl Hash for Replica {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.order.hash(state);
        self.content.hash(state);
    }
}

impl Eq for Replica {}

impl From<&Message> for Replica {
    fn from(msg: &Message) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            order: GLOBAL_ORDERING.fetch_add(1, Ordering::SeqCst),
            content: msg.content.clone(),
        }
    }
}

#[derive(Debug)]
pub struct ReplicatorMultiClient {
    nodes: Mutex<HashMap<String, Channel>>,
    in_sync: Mutex<HashMap<String, ac::Receiver<()>>>,
    stash: Mutex<HashSet<Replica>>,
    suspected_count: AtomicUsize,

}

impl ReplicatorMultiClient {

    const REQUEST_BLOCK_TIME_ON_SYNC_MS: u64 = 30000;

    pub fn init() -> Self {
        let nodes = Mutex::new(HashMap::new());
        let in_sync = Mutex::new(HashMap::new());
        let stash = Mutex::new(HashSet::new());
        let suspected_count = AtomicUsize::new(0);
        Self { nodes, stash, in_sync, suspected_count }
    }

    pub async fn verify_quorum(&self) -> bool {
        let nodes = self.nodes.lock().await;
        let active_nodes= nodes.len() - self.suspected_count.load(Ordering::Acquire);
        let meets = active_nodes >= *WRITE_QUORUM;
        if !meets {
            log::warn!(
                "The service is currently unavailable for storing messages due to possible consistency violations \
                (W={}, N={})", *WRITE_QUORUM, active_nodes
            );
        };

        meets
    }

    pub async fn replicate(&self, message: Message, request: RequestBody) {
        let wc = max(
            min(
                self.nodes.lock().await.len() + 1,
                request.wc as usize
            ),
            1,
        );

        let mut replica = Replica::from(&message);
        if let Some(order) = request.__ordering { replica.order = order }
        {
            let mut stash = self.stash.lock().await;
            stash.insert(replica.clone());
        }
        let wc_barrier = Arc::new(Barrier::new(wc));
        for (host, ch) in self.nodes.lock().await.iter() {
            let rep = replica.clone();
            let host = host.clone();
            let ch = ch.clone();
            let barrier = wc_barrier.clone();
            let sync_end_event = match self.in_sync.lock().await.get(&host) {
                Some(event) => Some(event.clone()),
                None => None
            };
            tokio::spawn(async move {
                if let Some(event) = sync_end_event {
                    Self::block_if_in_sync(&host, event).await
                }
                Self::replicate_per_node(rep, host, ch).await;
                barrier.wait().await;
            });
        }

        match wc {
            1 => log::info!("master: non-blocking replication... WRITE CONCERN - {wc}"),
            _ => log::info!("master: blocking replication... WRITE CONCERN - {wc}"),
        }
        wc_barrier.wait().await;
        log::info!("master: replication call completed")
    }

    async fn replicate_per_node(message: Replica, host: String, conn: Channel) {

        let content = message.content.clone();
        let mut att = Attempts::default();
        log::info!("{host}: {message:?} sent for replication");
        'done: {
            while att.next() {
                let mut client = ReplicatorClient::new(conn.clone());
                let request = Request::new(message.clone());
                let response = client.replicate(request).await;
                let success = match response {
                    Ok(body) => {
                        let ack = body.into_inner();
                        log::info!("{host}: message {content:?} - replication status - {ack:?}");
                        true
                    }
                    Err(e) => {
                        log::error!("{host}: message {content:?} - replication failed - {e:?}");
                        false
                    }
                };

                if success { break 'done }

                log::error!(
                    "{host}: request failed, retrying after {} ms, {} attempts left...",
                    att.backoff_ms.as_millis(),
                    att.n);

                att.delay().await

            }
            log::error!("{host}: message completely failed to replicate \
            (it will be stashed and re-processed again after the target node recovery)");
        }
    }

    pub async fn sync_node(&self, host: &str, channel: &Channel, from_order: u32) {
        let host = host.to_string();
        let futures: FuturesUnordered<JoinHandle<()>>;
        let (tx, rx) = async_channel::bounded(10);
        {
            let mut in_sync = self.in_sync.lock().await;
            in_sync.insert(host.clone(), rx);
        }
        {
            futures = self.stash.lock().await
                .iter()
                .filter(|msg| msg.order > from_order)
                .map(|msg|
                    tokio::spawn(
                        Self::replicate_per_node(
                            msg.clone(),
                            host.clone(),
                            channel.clone(),
                        )
                    )
                )
                .collect::<FuturesUnordered<_>>();
        }
        futures.for_each(|_| async {}).await;

        let _ = tx.send(()).await;
        let mut in_sync = self.in_sync.lock().await;
        in_sync.remove(&host);
    }

    async fn block_if_in_sync(host: &str, sync_end_event: ac::Receiver<()>) {
        log::info!("{host}: node in sync mode, blocked to process the replication");
        match timeout(
            Duration::from_millis(Self::REQUEST_BLOCK_TIME_ON_SYNC_MS),
            sync_end_event.recv()
        ).await {
            Err(_) => log::info!("{host}: the sync waiting time has elapsed, back to work"),
            _ => {}
        };
    }

    pub async fn add_node(&self, host: &str, channel: &Channel) {
        let mut nodes = self.nodes.lock().await;
        nodes.insert(host.to_string(), channel.clone());
    }

    pub async fn del_node(&self, name: &str) {
        let mut nodes = self.nodes.lock().await;
        nodes.remove(name);
    }

    pub fn increment_suspected(&self) {
        self.suspected_count.fetch_add(1, Ordering::Release);
    }

    pub fn decrement_suspected(&self) {
        if self.suspected_count.load(Ordering::Acquire) > 0 {
            self.suspected_count.fetch_sub(1, Ordering::Release);
        }
    }
}
