use anyhow::Context;
use reqwest::Client;
use serde_json::Value;

/// ZLM (ZLMediaKit) HTTP API 客户端
/// 仅保留压力测试所需功能：获取媒体列表、启动/停止 RTP 转发
#[derive(Debug, Clone)]
pub struct ZlmClient {
    client: Client,
    api_base: String,
    secret: String,
}

impl ZlmClient {
    /// 创建新的 ZLM 客户端
    pub fn new(api_base: String, secret: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
            api_base,
            secret,
        }
    }

    /// 为指定节点创建客户端（别名）
    pub fn for_node(api_base: &str, secret: &str) -> Self {
        Self::new(api_base.to_string(), secret.to_string())
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// 获取 ZLM 当前所有媒体流列表
    pub async fn get_media_list(&self) -> anyhow::Result<Vec<Value>> {
        let url = format!(
            "{}/index/api/getMediaList?secret={}",
            self.api_base, self.secret
        );
        let resp = self
            .retry(
                || async {
                    let r = self.client.get(&url).send().await?.json::<Value>().await?;
                    if r["code"].as_i64() != Some(0) {
                        anyhow::bail!("getMediaList failed: {:?}", r);
                    }
                    Ok(r)
                },
                2,
            )
            .await?;
        Ok(resp["data"].as_array().cloned().unwrap_or_default())
    }

    /// 主动发送 RTP 流（上级指定目标地址）
    pub async fn start_send_rtp(
        &self,
        vhost: &str,
        app: &str,
        stream: &str,
        ssrc: u32,
        dst_url: &str,
        dst_port: u16,
        is_udp: u8,
        src_port: Option<u16>,
        pt: Option<u8>,
        only_track: Option<u8>,
    ) -> anyhow::Result<u16> {
        let mut url = format!(
            "{}/index/api/startSendRtp?secret={}&vhost={}&app={}&stream={}&ssrc={}&dst_url={}&dst_port={}&is_udp={}",
            self.api_base, self.secret, vhost, app, stream, ssrc, dst_url, dst_port, is_udp
        );
        if let Some(sp) = src_port {
            url += &format!("&src_port={}", sp);
        }
        if let Some(p) = pt {
            url += &format!("&pt={}", p);
        }
        if let Some(ot) = only_track {
            url += &format!("&only_track={}", ot);
        }

        let resp = self
            .retry(
                || async {
                    let r = self.client.get(&url).send().await?.json::<Value>().await?;
                    if r["code"].as_i64() != Some(0) {
                        anyhow::bail!("startSendRtp failed: {:?}", r);
                    }
                    Ok(r)
                },
                2,
            )
            .await?;
        Ok(resp["local_port"].as_u64().context("no local_port")? as u16)
    }

    /// 被动发送 RTP 流（等待上级连接，ZLM 监听端口）
    pub async fn start_send_rtp_passive(
        &self,
        vhost: &str,
        app: &str,
        stream: &str,
        ssrc: u32,
        is_udp: u8,
        only_track: Option<u8>,
        only_audio: bool,
    ) -> anyhow::Result<u16> {
        let mut url = format!(
            "{}/index/api/startSendRtpPassive?secret={}&vhost={}&app={}&stream={}&ssrc={}&is_udp={}",
            self.api_base, self.secret, vhost, app, stream, ssrc, is_udp
        );
        if let Some(ot) = only_track {
            url += &format!("&only_track={}", ot);
        }
        if only_audio {
            url += "&only_audio=1";
        }

        let resp = self
            .retry(
                || async {
                    let r = self.client.get(&url).send().await?.json::<Value>().await?;
                    if r["code"].as_i64() != Some(0) {
                        anyhow::bail!("startSendRtpPassive failed: {:?}", r);
                    }
                    Ok(r)
                },
                2,
            )
            .await?;
        Ok(resp["local_port"].as_u64().context("no local_port")? as u16)
    }

    /// 停止 RTP 转发
    pub async fn stop_send_rtp(
        &self,
        vhost: &str,
        app: &str,
        stream: &str,
        ssrc: Option<u32>,
    ) -> anyhow::Result<()> {
        let mut url = format!(
            "{}/index/api/stopSendRtp?secret={}&vhost={}&app={}&stream={}",
            self.api_base, self.secret, vhost, app, stream
        );
        if let Some(s) = ssrc {
            url += &format!("&ssrc={}", s);
        }
        self.retry(
            || async {
                self.client.get(&url).send().await?;
                Ok(Value::Null)
            },
            1,
        )
        .await?;
        Ok(())
    }

    /// 内部重试辅助函数
    async fn retry<F, Fut>(&self, f: F, max_retries: u32) -> anyhow::Result<Value>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<Value>>,
    {
        let mut attempts = 0;
        let delay_ms = std::env::var("ZLM_RETRY_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);

        loop {
            match f().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    attempts += 1;
                    if attempts > max_retries {
                        return Err(e);
                    }
                    log::warn!(
                        "ZLM request failed (attempt {}/{}): {}",
                        attempts,
                        max_retries,
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }
}