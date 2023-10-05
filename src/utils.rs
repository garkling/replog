use std::any::Any;
use std::env;

use log4rs;
use actix_web::{web, App, HttpServer, HttpResponse};

pub fn init_logger() {
    let log_path = env::var("HOME").unwrap_or(String::from("."));
    let log_file = format!("{}/log-config.yml", log_path);

    log4rs::init_file(log_file, Default::default()).unwrap();

}
