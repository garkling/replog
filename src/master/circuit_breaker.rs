use std::collections::HashMap;
use std::time::Duration;
use std::env;

use tokio::select;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{channel, Sender};
use tonic::Request;
use tonic::transport::Channel;
use lazy_static::lazy_static;
use tokio::time::sleep;

use sync_request::SyncClaim;
use sync_request::sync_request_client::SyncRequestClient;

use crate::SharedReplicator;
use replog::common::heartbeats::{HealthChecker, HealthStatus};
use replog::common::retry::Attempts;

pub mod sync_request {
    tonic::include_proto!("syncreq");
}

lazy_static! {
    pub static ref STALL_NODE_LIFETIME_MS: Duration = Duration::from_millis(
        env::var("STALL_NODE_LIFETIME_MS")
            .unwrap_or_default()
            .parse()
            .unwrap_or(3600*1000)
    );
    pub static ref ABORT_CMD_TIMEOUT_MS: Duration = Duration::from_millis(
        env::var("ABORT_CMD_TIMEOUT_MS")
            .unwrap_or_default()
            .parse()
            .unwrap_or(10000)
    );
}

pub struct CircuitBreaker {
    client: SharedReplicator,
    stall: Mutex<HashMap<String, Sender<()>>>
}


impl CircuitBreaker {

    pub fn new(client: SharedReplicator) -> Self {
        Self {
            client,
            stall: Mutex::new(HashMap::new()),
        }
    }

    pub async fn watch(&self, node: Channel, name: String) {

        let (tx, mut rx) = channel::<HealthStatus>(10);
        let checker = HealthChecker::new(node.clone(), name.clone());
        let hb = tokio::spawn(
            async move { checker.ping(tx).await }
        );

        loop {
            match rx.recv().await {
                Some(HealthStatus::Healthy) => self.client.decrement_suspected(),
                Some(HealthStatus::Suspected) => self.client.increment_suspected(),
                _ => break,
            }
        }
        log::info!("{name}: breaking the connection");

        let (abort_tx, mut abort_rx) = channel::<()>(1);
        self.break_(&name, abort_tx).await;

        select! {
            _ = abort_rx.recv() => log::info!("{name}: node rejoined, aborting the old watcher..."),
            _ = sleep(*STALL_NODE_LIFETIME_MS) => {
                log::info!("{name}: node is in stall condition for too long, forgetting...");
                self.try_unwatch_old(&name).await;
            },
            res = hb => match res {
                Ok(_) => {
                    log::info!("{name}: node seems to be recovered, sending SyncRequest...");
                    self.initiate_sync(node, name).await;
                },
                Err(e) => log::error!("{name}: error happened during waiting for recovery - {e:?}")
            }
        }
    }

    pub async fn connect(&self, node: Channel, name: String, node_ordering: u32) {
        self.try_unwatch_old(&name).await;

        self.client.add_node(&name, &node).await;
        self.client.sync_node(&name, &node, node_ordering).await;
    }

    async fn break_(&self, node_name: &str, abort: Sender<()>) {
        {
            let mut stall = self.stall.lock().await;
            stall.insert(node_name.to_string(), abort);
        }

        self.client.del_node(node_name).await;
    }

    async fn initiate_sync(&self, node: Channel, name: String) {
        // a secondary node itself doesn't know that it has been stall or paused, so
        // the master needs to inform it and request its current state in order to synchronize the message log
        let mut att = Attempts::default();

        log::info!("{name}: initiating SyncRequest for the node to rejoin and sync...");
        while att.next() {
            let mut client = SyncRequestClient::new(node.clone());
            let request = Request::new(SyncClaim {});
            let res = match client.sync(request).await {
                Ok(_) => { log::info!("{name}: sync request sent"); true }
                Err(e) => { log::error!("{name}: SyncRequest failed - {e:?}"); false }
            };

            if res { break }
            log::error!(
                "{name}: request failed, retrying after {} ms, {} attempts left...",
                att.backoff_ms.as_millis(),
                att.n);

            att.delay().await
        }
    }

    async fn try_unwatch_old(&self, node_name: &str) {
        let mut stall = self.stall.lock().await;
        if let Some(abort) = stall.remove(node_name) {
            let _ = abort.send_timeout((), *ABORT_CMD_TIMEOUT_MS).await;
        }
    }
}
