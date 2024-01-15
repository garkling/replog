use lazy_static::lazy_static;
use std::env;

pub mod common {
    pub mod message;
    pub mod utils;
    pub mod retry;
    pub mod heartbeats;
}

lazy_static! {
    pub static ref RPC_DEF_PORT: u16 = env::var("RPC_PORT")
        .unwrap_or_default()
        .parse()
        .unwrap_or(50051);
    pub static ref SERVER_DEF_PORT: u16 = env::var("SERVER_PORT")
        .unwrap_or_default()
        .parse()
        .unwrap_or(10000);
    pub static ref SERVER_WORKER_NUM: usize = env::var("SERVER_WORKER_NUM")
        .unwrap_or_default()
        .parse()
        .unwrap_or(2);

    pub static ref REQ_TIMEOUT_MS: u64  = env::var("REQUEST_TIMEOUT_MS")
        .unwrap_or_default()
        .parse()
        .unwrap_or(120000);
    pub static ref RPC_SERVER_RECONNECT_DELAY_MS: u64  = env::var("RPC_SERVER_RECONNECT_DELAY_MS")
        .unwrap_or_default()
        .parse()
        .unwrap_or(4000);
    pub static ref WRITE_QUORUM: usize = env::var("WRITE_QUORUM")
        .unwrap_or_default()
        .parse()
        .unwrap_or(1);
}
