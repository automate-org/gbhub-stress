use crate::sip::config::SipConfig;
use crate::sip::session::SessionStore;
use crate::sip::utils::*;
use crate::stats::Stats;
use crate::zlm_client::ZlmClient;
use anyhow::Context;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, Mutex};

pub struct SipState {
    pub socket: Option<Arc<UdpSocket>>,
    pub local_tag: String,
    pub call_id_base: String,
    pub cseq: Arc<Mutex<u32>>,
    pub nonce: Arc<Mutex<Option<String>>>,
    pub config: SipConfig,
    pub zlm: Option<Arc<ZlmClient>>,
    pub sessions: SessionStore,
    pub peer_version: Arc<Mutex<String>>,
    pub public_ip: String,
    pub upper_server_id: String,
    pub registered_expires: AtomicU64,
    pub fixed_app: String,
    pub fixed_stream_name: String,
    pub heartbeat_interval: Duration,
    pub register_expires: u32,
    pub channel_id: String,
    pub stats: Arc<Stats>,
    shutdown_tx: broadcast::Sender<()>,
}

impl SipState {
    pub fn via_transport(&self) -> &str {
        if self.config.transport.eq_ignore_ascii_case("TCP") {
            "TCP"
        } else {
            "UDP"
        }
    }

    pub async fn new(
        config: SipConfig,
        zlm: Arc<ZlmClient>,
        sessions: SessionStore,
        fixed_app: String,
        fixed_stream_name: String,
        public_ip: String,
        heartbeat_interval: u64,
        register_expires: u32,
        stats: Arc<Stats>,
    ) -> anyhow::Result<Self> {
        let channel_id = config.channel_id.clone();
        let sock = UdpSocket::bind(config.local_addr).await?;
        let local_addr = sock.local_addr()?;
        let mut config = config;
        config.local_addr = local_addr;

        let upper_server_id = config
            .server_id
            .clone()
            .unwrap_or_else(|| config.device_id.clone());
        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            socket: Some(Arc::new(sock)),
            local_tag: uuid::Uuid::new_v4().to_string(),
            call_id_base: uuid::Uuid::new_v4().to_string(),
            cseq: Arc::new(Mutex::new(1)),
            nonce: Arc::new(Mutex::new(None)),
            config,
            zlm: Some(zlm),
            sessions,
            peer_version: Arc::new(Mutex::new("2016".to_string())),
            public_ip,
            upper_server_id,
            registered_expires: AtomicU64::new(register_expires as u64),
            fixed_app,
            fixed_stream_name,
            heartbeat_interval: Duration::from_secs(heartbeat_interval),
            register_expires,
            channel_id,
            stats,
            shutdown_tx,
        })
    }

    pub async fn send_bytes(&self, dest: SocketAddr, data: &[u8]) -> anyhow::Result<()> {
        if let Some(sock) = &self.socket {
            sock.send_to(data, dest).await?;
            Ok(())
        } else {
            anyhow::bail!("No UDP socket")
        }
    }

    pub async fn send_to(&self, dest: SocketAddr, data: &str) -> anyhow::Result<()> {
        self.send_bytes(dest, data.as_bytes()).await
    }

    pub async fn reply_response(
        &self,
        dest: SocketAddr,
        code: &str,
        headers: &[String],
        sdp: Option<&str>,
        to_tag: Option<&str>,
    ) -> anyhow::Result<()> {
        let version = self.peer_version.lock().await.clone();
        let via = get_header(headers, "Via").context("Via missing")?;
        let from = get_header(headers, "From").context("From missing")?;
        let to = get_header(headers, "To").context("To missing")?;
        let call_id = get_header(headers, "Call-ID").context("Call-ID missing")?;
        let cseq = get_header(headers, "CSeq").context("CSeq missing")?;
        let mut to_line = format!("To: {}{}", to, LINESEP);
        if let Some(tag) = to_tag {
            to_line = format!("To: {};tag={}{}", to, tag, LINESEP);
        }
        let mut msg = format!("SIP/2.0 {}{}", code, LINESEP);
        msg += &format!("Via: {}{}", via, LINESEP);
        msg += &format!("From: {}{}", from, LINESEP);
        msg += &to_line;
        msg += &format!("Call-ID: {}{}", call_id, LINESEP);
        msg += &format!("CSeq: {}{}", cseq, LINESEP);
        msg += &format!("X-GB-Ver: {}{}", gb_ver(&version), LINESEP);
        msg += "User-Agent: GBHub-Stress\r\n";
        if let Some(sdp_body) = sdp {
            msg += &format!(
                "Content-Type: application/sdp{}Content-Length: {}{}{}{}",
                LINESEP,
                sdp_body.len(),
                LINESEP,
                LINESEP,
                sdp_body
            );
        } else {
            msg += "Content-Length: 0\r\n\r\n";
        }
        self.send_to(dest, &msg).await?;
        Ok(())
    }

    pub async fn handle_catalog(
        &self,
        src: SocketAddr,
        _headers: &[String],
        body: &str,
    ) -> anyhow::Result<()> {
        let version = self.peer_version.lock().await.clone();
        let sn = extract_xml_text(body, "SN")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "1".to_string());
        let device_id = extract_xml_text(body, "DeviceID")
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config.device_id.clone());

        let channel_id = &self.channel_id;
        let channel_name = format!("Channel-{}", channel_id);

        let xml_body = format!(
            "{}<Response>\r\n\
             <CmdType>Catalog</CmdType>\r\n\
             <SN>{}</SN>\r\n\
             <DeviceID>{}</DeviceID>\r\n\
             <SumNum>1</SumNum>\r\n\
             <DeviceList Num=\"1\">\r\n\
             <Item>\r\n\
             <DeviceID>{}</DeviceID>\r\n\
             <Name>{}</Name>\r\n\
             <Parental>0</Parental>\r\n\
             <ParentID>{}</ParentID>\r\n\
             <Status>ON</Status>\r\n\
             </Item>\r\n\
             </DeviceList>\r\n\
             </Response>\r\n",
            xml_header(&version),
            sn,
            device_id,
            channel_id,
            channel_name,
            device_id
        );

        log::debug!(
            "Sending Catalog response to {}: SN={}, ChannelID={}",
            src,
            sn,
            channel_id
        );
        self.send_message_to(src, &xml_body).await?;
        Ok(())
    }

    pub async fn run_forever(self: Arc<Self>) -> anyhow::Result<()> {
        let mut reg_ok = false;
        for i in 0..5 {
            match self.register_with_retry().await {
                Ok(_) => {
                    reg_ok = true;
                    break;
                }
                Err(e) => {
                    log::error!("Initial REGISTER attempt {} failed: {}", i + 1, e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        if !reg_ok {
            anyhow::bail!("REGISTER failed after 5 retries");
        }

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut buf = vec![0u8; 65535];
        let mut heartbeat_interval = tokio::time::interval(self.heartbeat_interval);
        heartbeat_interval.tick().await;

        let self_clone = self.clone();
        let last_register = Arc::new(Mutex::new(tokio::time::Instant::now()));
        let heartbeat_failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        loop {
            tokio::select! {
                _ = heartbeat_interval.tick() => {
                    let self_ref = self_clone.clone();
                    let lr = last_register.clone();
                    let hf = heartbeat_failures.clone();
                    tokio::spawn(async move {
                        let mut retries = 3;
                        let mut success = false;
                        while retries > 0 && !success {
                            match self_ref.heartbeat().await {
                                Ok(_) => { success = true; hf.store(0, Ordering::Relaxed); }
                                Err(e) => {
                                    log::error!("Heartbeat error: {}", e);
                                    hf.fetch_add(1, Ordering::Relaxed);
                                    retries -= 1;
                                    if retries > 0 {
                                        tokio::time::sleep(Duration::from_secs(1)).await;
                                    }
                                }
                            }
                        }
                        if !success {
                            log::error!("Heartbeat failed after 3 retries");
                        }

                        let failures = hf.load(Ordering::Relaxed);
                        if failures < 5 {
                            let expires = self_ref.registered_expires.load(Ordering::Relaxed).max(60);
                            let re_register_interval = Duration::from_secs(expires / 4);
                            let mut last_reg = lr.lock().await;
                            if last_reg.elapsed() > re_register_interval {
                                match self_ref.register_with_retry().await {
                                    Ok(_) => { *last_reg = tokio::time::Instant::now(); }
                                    Err(e) => { log::error!("Re-reg failed: {}", e); }
                                }
                            }
                        }
                    });
                }

                result = async {
                    if let Some(sock) = &self.socket {
                        sock.recv_from(&mut buf).await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match result {
                        Ok((len, src)) => {
                            let raw_bytes = buf[..len].to_vec();
                            let data = String::from_utf8_lossy(&raw_bytes).to_string();
                            let s = self.clone();
                            tokio::spawn(async move {
                                if let Err(e) = s.process_incoming(src, data, &raw_bytes).await {
                                    log::error!("SIP error from {}: {}", src, e);
                                }
                            });
                        }
                        Err(e) => log::error!("recv error: {}", e),
                    }
                }

                _ = shutdown_rx.recv() => { break; }
            }
        }
        Ok(())
    }

    pub async fn process_incoming(
        &self,
        src: SocketAddr,
        data: String,
        _raw_bytes: &[u8],
    ) -> anyhow::Result<()> {
        // ---- 只处理来自上级 IP 的请求 ----
        let upstream_ip = self.config.sip_server.ip();
        if src.ip() != upstream_ip {
            log::debug!("Ignoring packet from non-upstream IP: {}", src);
            return Ok(());
        }

        // ---- 解析 SIP 消息 ----
        let (first_line, headers) = match parse_sip_lines(&data) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("Failed to parse SIP message from {}: {}", src, e);
                return Ok(());
            }
        };

        // ---- 处理 SIP 响应 ----
        if first_line.starts_with("SIP/2.0") {
            if first_line.contains("401 Unauthorized")
                || first_line.contains("407 Proxy Authentication Required")
            {
                if let Some(www_auth) = get_header(&headers, "WWW-Authenticate")
                    .or_else(|| get_header(&headers, "Proxy-Authenticate"))
                {
                    *self.nonce.lock().await = Some(www_auth.to_string());
                    let _ = self
                        .send_register_to(self.config.sip_server, self.register_expires, true)
                        .await;
                }
            }
            return Ok(());
        }

        // ---- 处理 SIP 请求 ----
        if let Some(ver) = get_header(&headers, "X-GB-Ver") {
            let ver = if ver.contains("3.0") { "2022" } else { "2016" };
            *self.peer_version.lock().await = ver.to_string();
        }

        if first_line.starts_with("INVITE") {
            let body = data.split("\r\n\r\n").nth(1).unwrap_or("");
            self.handle_invite(src, &headers, body).await?;
        } else if first_line.starts_with("BYE") {
            self.handle_bye(src, &headers).await?;
        } else if first_line.starts_with("ACK") {
            // ACK 无需回复
        } else if first_line.starts_with("OPTIONS") {
            log::debug!("Received OPTIONS, replying 200 OK");
            self.reply_response(src, "200 OK", &headers, None, None)
                .await?;
        } else if first_line.starts_with("CANCEL") {
            log::debug!("Received CANCEL, replying 200 OK");
            self.reply_response(src, "200 OK", &headers, None, None)
                .await?;
        } else if first_line.starts_with("MESSAGE") {
            let body = data.split("\r\n\r\n").nth(1).unwrap_or("");
            if body.contains("<CmdType>Catalog</CmdType>") {
                self.handle_catalog(src, &headers, body).await?;
            } else {
                log::debug!("Received non-Catalog MESSAGE, replying 200 OK");
                self.reply_response(src, "200 OK", &headers, None, None)
                    .await?;
            }
        } else {
            log::warn!("Received unsupported request: {}, replying 501", first_line);
            self.reply_response(src, "501 Not Implemented", &headers, None, None)
                .await?;
        }

        Ok(())
    }
}
