use crate::stats::Stats;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone)]
pub struct SipConfig {
    pub local_addr: SocketAddr,
    pub sip_server: SocketAddr,
    pub device_id: String,
    pub channel_id: String,
    pub realm: String,
    pub password: String,
    pub transport: String,
    pub server_id: Option<String>,
    #[allow(dead_code)]
    pub stats: Arc<Stats>,
}
