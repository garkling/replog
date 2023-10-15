use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::time;
use tonic::{transport::Server, Request, Response, Status};

use replicator::replicator_server::{Replicator, ReplicatorServer};
use replicator::{Replica, Ack};

use replog::common::message::{Message, MessageLog};

pub mod replicator {
    tonic::include_proto!("replica");
}


const REPL_DELAY: u8 = 5;


#[tonic::async_trait]
impl Replicator for MessageLog {

    async fn replicate(
        &self,
        request: Request<Replica>
    ) -> Result<Response<Ack>, Status> {

        let replica_msg: Replica = request.into_inner();
        log::info!("{:?} received", replica_msg);

        let message = Message {
            content: replica_msg.content
        };

        self.add(message.clone()).await;

        time::sleep(
            Duration::from_secs(REPL_DELAY as u64)
        ).await;

        log::info!("{:?} replicated", message);

        let response = Ack { status: true };

        Ok(Response::new(response))
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

    log::info!("Starting RPC server");
    if let Err(e) =  Server::builder()
        .add_service(ReplicatorServer::new(log))
        .serve(addr.unwrap())
        .await {
            panic!("RPC server failed - {e}");  // todo: implement server auto-recover
    }
}