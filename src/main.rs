mod sip;
mod stats;
mod zlm_client;

use dashmap::DashMap;
use log::info;
use sip::config::SipConfig;
use sip::session::SessionStore;
use sip::state::SipState;
use stats::Stats;
use std::net::SocketAddr;
use std::sync::Arc;
use zlm_client::ZlmClient;
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // ---- 读取环境变量 ----
    let upstream_ip = std::env::var("UPSTREAM_IP").expect("UPSTREAM_IP not set");
    let upstream_port: u16 = std::env::var("UPSTREAM_PORT")
        .unwrap_or_else(|_| "5060".to_string())
        .parse()?;
    let device_count: usize = std::env::var("DEVICE_COUNT")
        .unwrap_or_else(|_| "100".to_string())
        .parse()?;
    let base_port: u16 = std::env::var("BASE_PORT")
        .unwrap_or_else(|_| "15000".to_string())
        .parse()?;
    let device_id_prefix =
        std::env::var("DEVICE_ID_PREFIX").unwrap_or_else(|_| "3402000000".to_string());
    let realm = std::env::var("REALM").unwrap_or_else(|_| device_id_prefix.clone());
    let password = std::env::var("PASSWORD").unwrap_or_else(|_| "123456".to_string());
    let zlm_api = std::env::var("ZLM_API_BASE").expect("ZLM_API_BASE not set");
    let zlm_secret = std::env::var("ZLM_SECRET").expect("ZLM_SECRET not set");
    let fixed_stream =
        std::env::var("FIXED_STREAM").expect("FIXED_STREAM not set (e.g., rtp/test)");
    let public_ip = std::env::var("PUBLIC_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
    let heartbeat_interval: u64 = std::env::var("HEARTBEAT_INTERVAL")
        .unwrap_or_else(|_| "30".to_string())
        .parse()?;
    let register_expires: u32 = std::env::var("REGISTER_EXPIRES")
        .unwrap_or_else(|_| "3600".to_string())
        .parse()?;

    // ---- 解析固定流 ----
    let (app, stream) = fixed_stream
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("FIXED_STREAM must be in format 'app/stream'"))?;
    let fixed_app = app.to_string();
    let fixed_stream_name = stream.to_string();

    // ---- 创建 ZLM 客户端 ----
    let zlm_client = Arc::new(ZlmClient::new(zlm_api, zlm_secret));

    // ---- 创建会话存储 ----
    let sessions: SessionStore = Arc::new(DashMap::new());

    // ---- 创建统计实例（暂不启动定时打印） ----
    let stats = Arc::new(Stats::new(device_count as u64));

    // ---- 解析上级地址 ----
    let upstream_addr: SocketAddr = format!("{}:{}", upstream_ip, upstream_port).parse()?;

    info!(
        "GBHub-Stress starting: {} devices, base port {}, upstream {}",
        device_count, base_port, upstream_addr
    );
    info!("Fixed stream: {}", fixed_stream);
    info!("Public IP: {}", public_ip);

    let mut handles = Vec::with_capacity(device_count);

    for i in 0..device_count {
        let device_id = format!("{}{:010}", device_id_prefix, i + 1);
        let channel_id = format!("{}{:010}", device_id_prefix, i + 1 + 1000000);
        let local_port = base_port + i as u16;
        let local_addr = SocketAddr::from(([0, 0, 0, 0], local_port));

        let config = SipConfig {
            local_addr,
            sip_server: upstream_addr,
            device_id: device_id.clone(),
            channel_id,
            realm: realm.clone(),
            password: password.clone(),
            transport: "UDP".to_string(),
            server_id: Some(device_id.clone()),
            stats: stats.clone(),
        };

        let state = match SipState::new(
            config,
            zlm_client.clone(),
            sessions.clone(),
            fixed_app.clone(),
            fixed_stream_name.clone(),
            public_ip.clone(),
            heartbeat_interval,
            register_expires,
            stats.clone(),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to create device {}: {}", device_id, e);
                continue;
            }
        };

        let state = Arc::new(state);
        let device_id_clone = state.config.device_id.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = state.run_forever().await {
                log::error!("Device {} exited: {}", device_id_clone, e);
            }
        });
        handles.push(handle);

        if i % 100 == 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    // ---- 所有设备已启动 ----
    info!(
        "All {} devices started. Press Ctrl+C to stop.",
        device_count
    );

    // ---- 启动统计定时打印（每 30 秒） ----
    stats.clone().start_periodic_print(30);

    // ---- 等待 Ctrl+C ----
    tokio::signal::ctrl_c().await?;
    info!("Shutting down GBHub-Stress...");

    Ok(())
}
