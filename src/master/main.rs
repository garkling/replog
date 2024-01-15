use std::net::SocketAddr;
use std::sync::Arc;

use actix_web::{
    get, post, web,
    web::{Data, Json},
    App, HttpRequest, HttpResponse, HttpServer,
};
use log;
use serde::{Deserialize, Serialize};

use join_listener::JoinListener;
use replicator_client::ReplicatorMultiClient;

use replog::{common, SERVER_DEF_PORT, SERVER_WORKER_NUM};
use common::message::{Message, MessageLog};
use crate::circuit_breaker::CircuitBreaker;

mod join_listener;
mod replicator_client;
mod circuit_breaker;

pub type SharedReplicator = Arc<ReplicatorMultiClient>;


#[derive(Debug, Deserialize)]
pub struct RequestBody {
    message: String,
    wc: u8,  // write concern
    __ordering: Option<u32>,
}

#[derive(Serialize)]
struct ResponseBody {
    message: String,
    status: bool
}

#[post("/messages")]
async fn write_message(
    log: Data<MessageLog>,
    replicator_client: Data<SharedReplicator>,
    request: Json<RequestBody>,
    req: HttpRequest,
) -> HttpResponse {
    log::debug!("Called {} \"{}\" resource", req.method(), req.uri());

    if !replicator_client.verify_quorum().await {
        return HttpResponse::ServiceUnavailable().json(
            ResponseBody {
                message: String::from(
                    "The service cannot save the message due to a temporary failure/absence of the required nodes. \
                    Please try later."
                ),
                status: false,
            });
    }

    let request = request.into_inner();
    let message = Message {
        content: request.message.clone(),
    };

    log::info!("{:?} received", message);

    log.add(message.clone()).await;
    replicator_client.replicate(message, request).await;

    HttpResponse::Created().json(
        ResponseBody {
            status: true,
            message: String::from("Message delivered")
        })
}

#[get("/messages")]
async fn list_messages(log: Data<MessageLog>, req: HttpRequest) -> HttpResponse {
    log::debug!("Called {} \"{}\" resource", req.method(), req.uri());

    let messages = log.get_all().await;
    log::info!("Log has {} messages", messages.len());

    HttpResponse::Ok().json(messages)
}

pub fn config(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1")
            .service(write_message)
            .service(list_messages),
    );
}


#[actix_web::main]
async fn main() {
    common::utils::init_logger();

    log::info!("Initialized replication multi-client");
    let rep_client = Arc::new(ReplicatorMultiClient::init());
    let cb = CircuitBreaker::new(rep_client.clone());

    tokio::spawn(JoinListener::start(cb));

    let log = MessageLog::new();
    log::debug!("Initialized MessageLog object");

    let app_log = Data::new(log);

    let replicator_client = Data::new(rep_client);

    log::info!("Starting HTTP server");
    HttpServer::new(move || {
        App::new()
            .app_data(app_log.clone())
            .app_data(replicator_client.clone())
            .configure(config)
            .default_service(web::route().to(|| HttpResponse::MethodNotAllowed()))
    })
    .bind(SocketAddr::from(([0,0,0,0], *SERVER_DEF_PORT)))
    .expect("Failed to start a server")
    .workers(*SERVER_WORKER_NUM)
    .run()
    .await
    .expect("Server disconnected");
}
