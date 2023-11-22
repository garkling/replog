use std::cmp::max;
use std::env;
use std::net::SocketAddr;
use std::time::Duration;
use std::collections::HashSet;

use tokio::time;
use tokio::sync::RwLock;
use lazy_static::lazy_static;
use tonic::{transport::Server, Request, Response, Status};

use replicator::replicator_server::{Replicator, ReplicatorServer};
use replicator::{Replica, Ack};

use replog::common::message::{Message, MessageLog};

pub mod replicator {
    tonic::include_proto!("replica");
}


lazy_static! {

    static ref ORDER_DIFF_MULTIPLIER: f32 = env::var("ORDER_DIFF_MULTIPLIER")
        .unwrap_or_default()
        .parse::<f32>()
        .unwrap();

    static ref ORDER_CORRECTION_TIME_LIMIT: u8 = env::var("ORDER_CORRECTION_TIME_LIMIT_S")
        .unwrap_or_default()
        .parse::<u8>()
        .unwrap();

    static ref REPL_DELAY: u8 = env::var("REPLICATION_DELAY")
        .unwrap_or_default()
        .parse::<u8>()
        .unwrap_or(5);
}


#[derive(PartialEq)]
enum MessageStatus {
    Invalid,
    Belated,
    Correct,
    Disordered
}

struct ReplicatedMessageLog {
    pub state: ReplicationState,
    pub log: MessageLog,
}

struct ReplicationState {
    pub current_ordering: RwLock<u32>,
    pub unique_identifiers: RwLock<HashSet<String>>,
    pub messages_lost: RwLock<i8>
}


impl ReplicationState {

    pub fn new() -> Self {
        Self {
            current_ordering: RwLock::new(0),
            messages_lost: RwLock::new(0),
            unique_identifiers: RwLock::new(HashSet::new())
        }
    }

    pub async fn get_ordering(&self) -> u32 {
        *self.current_ordering.read().await
    }

    pub async fn duplicates(&self, identifier: &String) -> bool {
        let set = self.unique_identifiers.read().await;
        set.contains(identifier)
    }

    pub async fn consecutive_ordering(&self, order: u32) -> bool {
        order - 1 == self.get_ordering().await
    }

    pub async fn has_lost_messages(&self) -> bool {
        *self.messages_lost.read().await != 0
    }

    pub async fn register_id(&self, identifier: String) {
        let mut set = self.unique_identifiers.write().await;
        set.insert(identifier);
    }

    pub async fn register_ordering(&self, ordering: u32) {
        let mut go = self.current_ordering.write().await;
        *go = ordering;
    }

    pub async fn modify_lost_message_count(&self, modifier: i8) {
        let mut lost = self.messages_lost.write().await;
        *lost = max(0, *lost + modifier)
    }

}


impl ReplicatedMessageLog {

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
        let curr_order = self.state.get_ordering().await;
        if msg_ordering <= curr_order {
            if self.state.has_lost_messages().await {

                log::info!("The lost message with ordering ({msg_ordering}) found!");
                self.state.modify_lost_message_count(-1).await;
                MessageStatus::Belated

            } else {
                log::warn!(
                    "The message ordering ({msg_ordering}) is repeating according to the global one ({curr_order}). Aborting..."
                );
                MessageStatus::Invalid
            }
        } else if !(self.state.consecutive_ordering(msg_ordering).await) {
            log::info!(
                "The message ordering ({msg_ordering}) does not correspond with the global one ({curr_order}). Waiting for its turn..."
            );
            MessageStatus::Disordered

        } else {
            MessageStatus::Correct
        }
    }

    pub async fn correct_ordering(&self, msg_ordering: u32) {
        let mut timeout: u8 = 0;
        let order_diff = msg_ordering - self.state.get_ordering().await;
        let modifier = ( // bigger difference = more time for the order correction
            *ORDER_CORRECTION_TIME_LIMIT as f32 *
                (*ORDER_DIFF_MULTIPLIER * (order_diff - 2) as f32)
        ) as u8;

        let total_timeout = *ORDER_CORRECTION_TIME_LIMIT + modifier;

        while !(self.state.consecutive_ordering(msg_ordering).await) {
            timeout += 1;
            time::sleep(Duration::from_secs(1)).await;

            if timeout > total_timeout {
                log::warn!("The awaiting message(s) have been lost. Continue processing the current queue");
                self.state.modify_lost_message_count((order_diff - 1) as i8).await;
                break;
            }
        };
    }
}


#[tonic::async_trait]
impl Replicator for ReplicatedMessageLog {

    async fn replicate(
        &self,
        request: Request<Replica>
    ) -> Result<Response<Ack>, Status> {

        let replica_msg: Replica = request.into_inner();
        log::info!("{:?} received", replica_msg);

        let status = self.validate(&replica_msg).await;
        if status == MessageStatus::Invalid {
            return Ok(
                Response::new(Ack { status: false })
            )
        }

        self.state.register_id(replica_msg.id.clone()).await;
        if status == MessageStatus::Disordered {
            self.correct_ordering(replica_msg.order).await;
        }

        time::sleep(
            Duration::from_secs(*REPL_DELAY as u64)
        ).await;

        let message = Message { content: replica_msg.content };
        self.log.add(message.clone()).await;
        log::info!("{:?} replicated", message);

        if !(status == MessageStatus::Belated) {
            self.state.register_ordering(replica_msg.order).await;
        }

        Ok(
            Response::new(Ack { status: true })
        )
    }
}

pub async fn start(log: MessageLog) {

    let addr = format!(
        "[::0]:{}",
        env::var("RPC_PORT")
            .unwrap_or(String::from(replog::PRC_DEF_PORT)));

    let addr = addr.parse::<SocketAddr>();
    if let Err(e) = addr {
        panic!("Invalid socket address - {e}");
    }

    let repl_log = ReplicatedMessageLog { state: ReplicationState::new(), log };

    log::info!("Starting RPC server");
    if let Err(e) =  Server::builder()
        .add_service(ReplicatorServer::new(repl_log))
        .serve(addr.unwrap())
        .await {
            panic!("RPC server failed - {e}");  // todo: implement server auto-recover
    }
}