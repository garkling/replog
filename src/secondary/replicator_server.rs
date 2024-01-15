use std::env;
use std::time::Duration;
use std::cmp::{max, min};
use std::net::SocketAddr;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU32, Ordering};

use lazy_static::lazy_static;
use tokio::sync::RwLock;
use tokio::time::{interval, sleep, timeout};
use tonic_health::pb::health_server::HealthServer;
use tonic::{transport::Server, Request, Response, Status, async_trait};

use replicator::{Ack, Replica};
use replicator::replicator_server::{Replicator, ReplicatorServer};

use sync_request::{EmptyAck, SyncClaim};
use sync_request::sync_request_server::{SyncRequest, SyncRequestServer};

use replog::common::message::{Message, MessageLog};
use replog::{RPC_DEF_PORT, REQ_TIMEOUT_MS, RPC_SERVER_RECONNECT_DELAY_MS};
use replog::common::heartbeats::HealthService;
use crate::join_requester::try_join;
use crate::SABOTAGE_MODE;

pub mod replicator {
    tonic::include_proto!("replica");
}
pub mod sync_request {
    tonic::include_proto!("syncreq");
}

type ReplReq = Request<Replica>;
type ReplRes = Result<Response<Ack>, Status>;
type SyncReq = Request<SyncClaim>;
type SyncRes = Result<Response<EmptyAck>, Status>;

lazy_static! {
    static ref ORDER_DIFF_MULTIPLIER: f32 = env::var("ORDER_DIFF_MULTIPLIER")
        .unwrap_or_default()
        .parse()
        .unwrap_or(0.2);
    static ref ORDER_CORRECTION_TIME_LIMIT_MS: u64 = env::var("ORDER_CORRECTION_TIME_LIMIT_MS")
        .unwrap_or_default()
        .parse()
        .unwrap_or(60000);
    static ref REPL_DELAY_MS: Duration = Duration::from_millis(
        env::var("REPLICATION_DELAY_MS")
            .unwrap_or_default()
            .parse()
            .unwrap_or(5000)
    );
}


#[derive(PartialEq, Debug)]
pub enum MessageStatus {
    Invalid,
    Belated,
    Correct,
    Disordered,
}

#[derive(Debug, Default)]
pub struct SyncMode(AtomicBool);

impl SyncMode {

    pub fn enabled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn toggle(&self, mode: bool) {
        let _ = self.0.compare_exchange(
            !mode,
            mode,
            Ordering::Release,
            Ordering::Acquire,
        );
    }
}


pub struct ReplicatedMessageLog {
    pub state: ReplicationState,
    pub log: MessageLog
}

#[derive(Debug, Clone)]
pub struct ReplicationState {
    pub current_ordering: Arc<AtomicU32>,
    pub messages_lost: Arc<AtomicI8>,
    pub unique_identifiers: Arc<RwLock<HashSet<String>>>,
    pub sync_mode: Arc<SyncMode>
}

impl ReplicationState {
    pub fn new() -> Self {
        Self {
            current_ordering: Arc::new(AtomicU32::new(0)),
            messages_lost: Arc::new(AtomicI8::new(0)),
            unique_identifiers: Arc::new(RwLock::new(HashSet::new())),
            sync_mode: Arc::new(SyncMode::default())
        }
    }

    pub fn get_ordering(&self) -> u32 {
        self.current_ordering.load(Ordering::Acquire)
    }

    pub async fn duplicates(&self, identifier: &String) -> bool {
        let set = self.unique_identifiers.read().await;
        set.contains(identifier)
    }

    pub fn consecutive_ordering(&self, order: u32) -> bool {
        order - 1 == self.get_ordering()
    }

    pub fn has_lost_messages(&self) -> bool {
        self.messages_lost.load(Ordering::Acquire) != 0
    }

    pub async fn register_id(&self, identifier: String) {
        let mut set = self.unique_identifiers.write().await;
        set.insert(identifier);
    }

    pub fn register_ordering(&self, ordering: u32) {
        self.current_ordering.store(ordering, Ordering::Release);
    }

    pub fn modify_lost_message_count(&self, modifier: i8) {
        let _ = self.messages_lost.fetch_update(
            Ordering::Release,
            Ordering::Acquire,
            |n| Some(max(0, n + modifier))
        );
    }
}


impl From<&ReplicatedMessageLog> for ReplicatedMessageLog {
    fn from(rml: &Self) -> Self {
        Self {
            state: rml.state.clone(),
            log: MessageLog::from(&rml.log)
        }
    }
}


impl ReplicatedMessageLog {

    pub fn new() -> Self {
        Self {
            log: MessageLog::new(),
            state: ReplicationState::new()
        }
    }

    pub async fn validate(&self, msg: &Replica) -> MessageStatus {
        if self.validate_uniqueness(&msg.id).await == MessageStatus::Correct {
            self.validate_ordering(msg.order).await
        } else {
            MessageStatus::Invalid
        }
    }

    async fn validate_uniqueness(&self, msg_id: &String) -> MessageStatus {
        if self.state.duplicates(msg_id).await {
            log::warn!("Message duplication detected: {msg_id}");
            MessageStatus::Invalid
        } else {
            MessageStatus::Correct
        }
    }

    async fn validate_ordering(&self, msg_ordering: u32) -> MessageStatus {
        let curr_order = self.state.get_ordering();
        if msg_ordering <= curr_order {
            if self.state.has_lost_messages() {
                log::info!("The lost message with ordering ({msg_ordering}) found!");
                self.state.modify_lost_message_count(-1);
                MessageStatus::Belated
            } else {
                log::warn!(
                    "The message ordering ({msg_ordering}) is repeating according to the global one ({curr_order}). Aborting..."
                );
                MessageStatus::Invalid
            }
        } else if !(self.state.consecutive_ordering(msg_ordering)) {
            log::info!(
                "The message ordering ({msg_ordering}) does not correspond with the global one ({curr_order}). Waiting for its turn..."
            );
            MessageStatus::Disordered
        } else {
            MessageStatus::Correct
        }
    }

    pub async fn correct_ordering(&self, msg_ordering: u32) {
        let order_diff = msg_ordering - self.state.get_ordering();
        let modifier = (
            // bigger difference = more time for the order correction
            *ORDER_CORRECTION_TIME_LIMIT_MS as f32 * (*ORDER_DIFF_MULTIPLIER * (order_diff - 2) as f32)
        ) as u64;

        let max_correction_time = min(
            (*REQ_TIMEOUT_MS) - 10000,
            *ORDER_CORRECTION_TIME_LIMIT_MS + modifier
        );

        let mut inter = interval(Duration::from_secs(1));
        match timeout(
            Duration::from_millis(max_correction_time),
            async {
                while !(self.state.consecutive_ordering(msg_ordering)) {
                    inter.tick().await;
                }
            }
        ).await {
            Err(_) => {
                log::warn!("The awaiting message(s) have been lost. Continue processing the current queue");
                self.state.modify_lost_message_count((order_diff - 1) as i8);
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Replicator for ReplicatedMessageLog {

    async fn replicate(&self, request: ReplReq) -> ReplRes {

        let replica_msg: Replica = request.into_inner();
        log::info!("{:?} received", replica_msg);

        let status = self.validate(&replica_msg).await;
        if status == MessageStatus::Invalid {
            return Ok(Response::new(Ack { success: false }));
        }

        self.state.register_id(replica_msg.id.clone()).await;
        if status == MessageStatus::Disordered {
            self.correct_ordering(replica_msg.order).await;
        }

        sleep(*REPL_DELAY_MS).await;

        let message = Message {
            content: replica_msg.content,
        };
        self.log.add(message.clone()).await;
        log::info!("{:?} replicated", message);

        if !(status == MessageStatus::Belated) {
            self.state.register_ordering(replica_msg.order);
        }

        match SABOTAGE_MODE.load(Ordering::Acquire) {
            false => Ok(Response::new(Ack { success: true })),
            true => Err(Status::internal("Internal server error"))
        }
    }
}

#[async_trait]
impl SyncRequest for ReplicatedMessageLog {
    async fn sync(&self, _: SyncReq) -> SyncRes {
        let ordering = self.state.get_ordering();

        if !self.state.sync_mode.enabled() {
            let mode = self.state.sync_mode.clone();
            mode.toggle(true);
            tokio::spawn(async move {
                try_join(ordering).await;
                mode.toggle(false)
            });
        }

        Ok(
            Response::new(EmptyAck {})
        )
    }
}


pub async fn start(repl_log: ReplicatedMessageLog) {
    let addr = SocketAddr::from(([0, 0, 0, 0], *RPC_DEF_PORT));

    let dur = Duration::from_millis(*RPC_SERVER_RECONNECT_DELAY_MS);
    loop {
        log::info!("Starting replication server");
        let health_service = HealthService {};
        match Server::builder()
            .timeout(Duration::from_millis(*REQ_TIMEOUT_MS))
            .add_service(HealthServer::new(health_service))
            .add_service(ReplicatorServer::new(ReplicatedMessageLog::from(&repl_log)))
            .add_service(SyncRequestServer::new(ReplicatedMessageLog::from(&repl_log)))
            .serve(addr)
            .await {
            Ok(_) => break,
            Err(e) => {
                log::error!("ReplicatorServer failed - {e:?}. Reconnect after {} ms...", dur.as_millis());

                sleep(dur).await;
            }
        }
    }
}