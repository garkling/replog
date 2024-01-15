use std::fs;

use log;
use tonic::transport::Endpoint;
use tonic::Request;

use join_request::join_request_client::JoinRequestClient;
use join_request::NodeState;
use replog::common::retry::Attempts;
use replog::RPC_DEF_PORT;

pub mod join_request {
    tonic::include_proto!("joinreq");
}

const MASTER_HOSTNAME: &str = "master";

pub async fn try_join(ordering: u32) -> bool {

    let master_url =
        Endpoint::from_shared(format!("http://{}:{}", MASTER_HOSTNAME, *RPC_DEF_PORT)).unwrap();

    let host = get_hostname().unwrap_or_default();
    let info = NodeState { host, ordering };

    let mut att = Attempts::default();
    log::info!("Joining to the master with the current message ordering ({ordering})...");

    while att.next() {
        let request = Request::new(info.clone());
        let res = match JoinRequestClient::connect(master_url.clone()).await {
            Ok(mut client) => match client.join(request).await {
                Ok(body) => {
                    let response = body.into_inner();
                    log::info!("Joining to the master status - {response:?}");
                    response.success
                }
                Err(e) => {
                    log::error!("JoinRequest failed - {e:?}");
                    false
                }
            }
            Err(e) => {
                log::error!("Connection to the master failed - {e:?}");
                false
            }
        };

        if res { return res }
        log::error!(
            "Request failed, retrying after {} ms, {} attempts left...",
            att.backoff_ms.as_millis(),
            att.n);

        att.delay().await
    }

    false
}

fn get_hostname() -> Option<String> {
    match fs::read_to_string("/etc/hostname") {
        Ok(host) => Some(host.trim().to_string()),
        Err(_) => None,
    }
}
