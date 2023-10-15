mod replicator_server;

use log;
use actix_web::{web::{Data}, web, get, App, HttpResponse, HttpServer, HttpRequest};

use replog::common;
use common::message::MessageLog;


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
            .service(list_messages)
    );
}


#[actix_web::main]
async fn main() {

    common::utils::init_logger();

    let msg_log = MessageLog::new();
    let rpc_log = MessageLog::from(&msg_log);
    log::debug!("Initialized MessageLog object");

    tokio::spawn( async move {
            replicator_server::start(rpc_log).await
        }
    );

    let app_log = Data::new(msg_log);

    log::info!("Starting HTTP server");
    HttpServer::new(move || App::new()
        .app_data(app_log.clone())
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