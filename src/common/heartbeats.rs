use std::env;
use std::time::Duration;
use lazy_static::lazy_static;
use tokio::sync::mpsc::Sender;
use tokio::time::interval;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tonic::transport::Channel;
use tonic_health::pb::health_server::Health;
use tonic_health::pb::{HealthCheckRequest, HealthCheckResponse};
use tonic_health::pb::health_client::HealthClient;
use tonic_health::ServingStatus;

use crate::common::heartbeats::HealthStatus::*;


lazy_static! {
    static ref FAIL_BUDGET: u8 = env::var("HB_FAIL_BUDGET")
        .unwrap_or_default()
        .parse()
        .unwrap_or(5);
    static ref HB_INTERVAL_MS: Duration = Duration::from_millis(
        env::var("HB_INTERVAL_MS")
            .unwrap_or_default()
            .parse()
            .unwrap_or(5000)
    );
    static ref HB_REQUEST_TIMEOUT_MS: Duration = Duration::from_millis(
        env::var("HB_REQUEST_TIMEOUT_MS")
            .unwrap_or_default()
            .parse()
            .unwrap_or(3000)
    );
    static ref POST_FAIL_INTERVAL_MS: Duration = Duration::from_millis(
        env::var("POST_FAIL_INTERVAL_MS")
            .unwrap_or_default()
            .parse()
            .unwrap_or(60000)
    );
}


#[derive(PartialEq, Copy, Clone)]
pub enum HealthStatus {
    Healthy,
    Suspected,
    Failed
}

pub struct HealthService;

pub struct HealthChecker {
    pub service: Channel,
    pub service_name: String
}

#[async_trait]
impl Health for HealthService {
    async fn check(&self, _: Request<HealthCheckRequest>) -> Result<Response<HealthCheckResponse>, Status> {
        let response = HealthCheckResponse {
            status: ServingStatus::Serving as i32
        };

        Ok(Response::new(response))
    }

    type WatchStream = Streaming<HealthCheckResponse>;

    async fn watch(&self, _: Request<HealthCheckRequest>) -> Result<Response<Self::WatchStream>, Status> {
        unimplemented!()
    }
}

impl HealthChecker {

    const FAILURE_ACK_TIMEOUT_MS: Duration = Duration::from_millis(10000);

    pub fn new(service: Channel, service_name: String) -> Self {
        Self {service, service_name}
    }

    pub async fn ping(&self, tx: Sender<HealthStatus>) {
        let mut client = HealthClient::new(self.service.clone());

        let mut status = Healthy;
        let mut budget = *FAIL_BUDGET;
        let mut inter = interval(*HB_INTERVAL_MS);
        inter.tick().await;

        let request = HealthCheckRequest { service: self.service_name.clone() };
        loop {
            let mut request = Request::new(request.clone());
            request.set_timeout(*HB_REQUEST_TIMEOUT_MS);

            match client.check(request).await {
                Ok(_) => {
                    if status == Failed { break }
                    if budget != *FAIL_BUDGET {
                        budget = *FAIL_BUDGET;
                        status = Healthy;
                        let _ = tx.send_timeout(status, Self::FAILURE_ACK_TIMEOUT_MS).await;
                    }
                }
                Err(response) => {
                    if budget > 0 {
                        budget -= 1;
                        if status != Suspected {
                            status = Suspected;
                            let _ = tx.send_timeout(status, Self::FAILURE_ACK_TIMEOUT_MS).await;
                        }
                    } else {
                        status = Failed;
                        match tx.send_timeout(status, Self::FAILURE_ACK_TIMEOUT_MS).await {
                            Ok(_) => {
                                log::error!("{}: health checks failed - {response}", self.service_name);
                                inter = interval(*POST_FAIL_INTERVAL_MS);
                                inter.tick().await;
                            }
                            _ => {}
                        }
                    }
                }
            };

            inter.tick().await;
        }
    }
}
