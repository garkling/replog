use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tonic::{transport::Server, Request, Response, Status, async_trait};
use tonic::transport::Endpoint;

use join_request::join_request_server::{JoinRequest, JoinRequestServer};
use join_request::{Ack, NodeState};

use replog::{RPC_DEF_PORT, RPC_SERVER_RECONNECT_DELAY_MS};
use crate::circuit_breaker::CircuitBreaker;

pub mod join_request {
    tonic::include_proto!("joinreq");
}

pub struct JoinListener {
    cb: Arc<CircuitBreaker>
}

#[async_trait]
impl JoinRequest for JoinListener {
    async fn join(&self, request: Request<NodeState>) -> Result<Response<Ack>, Status> {
        let remote_addr = request.remote_addr();
        let body = request.into_inner();
        log::info!("Received JoinRequest - {body:?}");

        let url = if body.host.is_empty() {
            let addr = remote_addr.ok_or(
                    Status::invalid_argument("Cannot fetch a valid resource to connect to")
            )?;
            format!("http://{}:{}", addr.ip(), *RPC_DEF_PORT)
        } else {
            format!("http://{}:{}", body.host, *RPC_DEF_PORT)
        };

        let success = match Endpoint::from_shared(url) {
            Ok(end) => {
                let host = end.uri().host().unwrap();
                let channel = end
                    .clone()
                    .connect_timeout(Duration::from_secs(10))
                    .connect()
                    .await;

                match channel {
                    Ok(channel) => {
                        let host = host.to_string();
                        let cb = self.cb.clone();
                        cb.connect(channel.clone(), host.clone(), body.ordering).await;

                        tokio::spawn(async move {
                            cb.watch(channel, host).await
                        });

                        log::info!("Secondary end `{}` connected", end.uri());
                        true
                    },
                    Err(e) => {
                        log::error!("JoinRequest error happened - {e:?}");
                        false
                    }
                }
            },
            Err(e) => {
                log::error!("JoinRequest error happened - {e:?}");
                false
            }
        };

        Ok(Response::new(Ack { success }))

    }
}


impl JoinListener {

    pub async fn start(cb: CircuitBreaker) {

        let addr = SocketAddr::from(([0,0,0,0], *RPC_DEF_PORT));

        let dur = Duration::from_millis(*RPC_SERVER_RECONNECT_DELAY_MS);
        let cb = Arc::new(cb);
        loop {
            match Server::builder()
                .add_service(JoinRequestServer::new(Self { cb: cb.clone() }))
                .serve(addr)
                .await {

                Ok(_) => break,
                Err(e) => {
                    log::error!("JoinRequestServer failed - {e:?}. Reconnect after {} ms...", dur.as_millis());

                    sleep(dur).await;
                }
            }
        }
    }
}