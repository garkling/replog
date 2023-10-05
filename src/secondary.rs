use std::sync::Arc;
use std::net::SocketAddr;

use log;
use log4rs;
use actix_web::{web::{Data, Json}, web, post, get, App, HttpResponse, HttpServer, HttpRequest};
use tonic::{transport::Server, Request, Response, Status};

use replicator::replicator_server::{Replicator, ReplicatorServer};
use replicator::{Replica, Acknowledge};
use message::{Message, MessageLog};

mod message;
mod utils;

pub mod replicator {
    tonic::include_proto!("replica");
}


#[tonic::async_trait]
impl Replicator for MessageLog {

    async fn replicate(
        &self,
        request: Request<Replica>
    ) -> Result<Response<Acknowledge>, Status> {

        log::info!("RPC request received: {:?}", request);

        let replica_content: String = request.into_inner().content;

        let message = Message {
            content: replica_content.clone()
        };

        self.add(message).await;

        log::info!("Message `{}` replicated", replica_content);

        let response = Acknowledge { status: true };

        Ok(Response::new(response))
    }
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
            .service(list_messages)
    );
}


#[actix_web::main]
async fn main() {

    utils::init_logger();

    let rpc_addr: SocketAddr = match "[::0]:50051".parse() {
        Ok(v) => v,
        Err(e) => {
            log::error!("Invalid socket address - {e}");
            return
        }
    };

    let rpc_log = MessageLog::new();
    let app_log = MessageLog::from(&rpc_log);

    log::debug!("Initialized MessageLog object");

    tokio::spawn(async move {
            match Server::builder()
                .add_service(ReplicatorServer::new(rpc_log))
                .serve(rpc_addr)
                .await {
                Err(e) => log::error!("RPC server failed - {e}"),
                _ => {}

            }
        }
    );

    let app_data = Data::new(app_log);

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