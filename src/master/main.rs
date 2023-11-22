use std::env;
use std::sync::Arc;

use log;
use serde::{Deserialize, Serialize};
use actix_web::{web::{Data, Json}, web, post, get, App, HttpResponse, HttpServer, HttpRequest};

use replicator_client::ReplicatorMultiClient;

use replog::common;
use common::message::{Message, MessageLog};

mod replicator_client;

#[derive(Debug, Deserialize)]
struct RequestBody {
    message: String,
    wc: u8,  // write concern
    __ordering: Option<u32>,
    #[serde(default)]
    __duplicate: bool,
}


#[derive(Serialize)]
struct ResponseBody {
    status: bool
}


#[post("/messages")]
async fn write_message(
    log: Data<MessageLog>,
    replicator_client: Data<Arc<ReplicatorMultiClient>>,
    request: Json<RequestBody>,
    req: HttpRequest) -> HttpResponse {

    log::debug!("Called {} \"{}\" resource", req.method(), req.uri());

    let request = request.into_inner();
    let message = Message { content: request.message.clone() };

    log::info!("{:?} received", message);

    log.add(message.clone()).await;

    replicator_client.replicate(message, request).await;

    HttpResponse::Created().json(ResponseBody { status: true })

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
            .service(list_messages)
    );
}


#[actix_web::main]
async fn main() {

    common::utils::init_logger();

    let hosts: Vec<String> = match env::var("SECONDARY_HOSTS") {
        Ok(v) => v.split(',').map(str::to_string).collect(),
        Err(_) => vec![]
    };

    let hosts = hosts.iter()
        .map(|host| format!(
            "http://{}:{}", host,
            env::var("RPC_PORT")
                .unwrap_or(String::from(replog::PRC_DEF_PORT))
            )
        );

    let log = MessageLog::new();
    log::debug!("Initialized MessageLog object");

    let app_log = Data::new(log);

    let replicator_client = ReplicatorMultiClient::init(hosts);
    log::info!("Initialized {replicator_client:?}");

    let replicator_client = Data::new(
        Arc::new(replicator_client)
    );

    log::info!("Starting HTTP server");
    HttpServer::new(move || App::new()
        .app_data(app_log.clone())
        .app_data(replicator_client.clone())
        .configure(config)
        .default_service(
            web::route().to(|| HttpResponse::MethodNotAllowed())
        )
    )
    .bind("[::0]:10000").expect("Failed to start a server")
    .workers(2)
    .run()
    .await
    .expect("Server disconnected");

}