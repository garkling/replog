mod join_requester;
mod replicator_server;

use std::net::SocketAddr;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};

use actix_web::{get, web, web::Data, App, HttpRequest, HttpResponse, HttpServer, post};
use log;

use crate::join_requester::try_join;
use common::message::MessageLog;
use replog::{common, SERVER_DEF_PORT, SERVER_WORKER_NUM};
use crate::replicator_server::ReplicatedMessageLog;


static SABOTAGE_MODE: AtomicBool = AtomicBool::new(false);


#[get("/messages")]
async fn list_messages(log: Data<MessageLog>, req: HttpRequest) -> HttpResponse {
    log::debug!("Called {} \"{}\" resource", req.method(), req.uri());

    let messages = log.get_all().await;
    log::info!("Log has {} messages", messages.len());

    HttpResponse::Ok().json(messages)
}

#[post("/sabotage")]
async fn __sabotage() -> HttpResponse {
    let current = SABOTAGE_MODE.load(Ordering::SeqCst);
    SABOTAGE_MODE.store(!current, Ordering::SeqCst);
    log::info!("Sabotage? {}", !current);
    HttpResponse::Ok().json("The sabotage mode switched")
}

pub fn config(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1")
            .service(list_messages)
            .service(__sabotage)
    );
}

#[actix_web::main]
async fn main() {
    common::utils::init_logger();

    let repl_log = ReplicatedMessageLog::new();
    log::debug!("Initialized ReplicatedMessageLog object");

    tokio::spawn(replicator_server::start((&repl_log).into()));

    repl_log.state.sync_mode.toggle(true);
    if !try_join(0).await {
        log::error!("Failed to establish a connection with the master node, abort");
        exit(1)
    }
    repl_log.state.sync_mode.toggle(false);

    let msg_log = MessageLog::from(&repl_log.log);
    let app_log = Data::new(msg_log);
    log::info!("Starting HTTP server");
    HttpServer::new(move || {
        App::new()
            .app_data(app_log.clone())
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
