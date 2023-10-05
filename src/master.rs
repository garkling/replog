mod message;
mod utils;

use std::env;

use log;
use log4rs;
use actix_web::{web::{Data, Json}, web, post, get, App, HttpResponse, HttpServer, HttpRequest};

use message::{Message, MessageLog};

#[post("/messages")]
async fn write_message(log: Data<MessageLog>, new_msg: Json<Message>, req: HttpRequest) -> HttpResponse {
    log::debug!("Called {} \"{}\" resource", req.method(), req.uri());

    let message = new_msg.into_inner();

    log::info!("Message `{}` received", &message.content);

    log.add(message).await;

    HttpResponse::Ok().body("Success")

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

    utils::init_logger();

    let m_log = MessageLog::new();
    log::debug!("Initialized MessageLog object");

    let app_data = Data::new(m_log);

    log::info!("Starting HTTP server");
    HttpServer::new(move || App::new()
        .app_data(app_data.clone())
        .configure(config)
        .default_service(
            web::route().to(|| HttpResponse::MethodNotAllowed())
        )
    )
    .bind("0.0.0.0:10000").expect("Failed to start a server")
    .workers(2)
    .run()
    .await
    .expect("Server disconnected");

}