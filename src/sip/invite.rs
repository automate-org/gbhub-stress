use crate::sip::session::{add_session, get_session, remove_session, SessionInfo};
use crate::sip::state::SipState;
use crate::sip::utils::*;
use anyhow::Context;
use std::net::SocketAddr;

impl SipState {
    pub async fn handle_invite(
        &self,
        src: SocketAddr,
        headers: &[String],
        body: &str,
    ) -> anyhow::Result<()> {
        // 记录 INVITE
        self.stats.mark_invite(&self.config.device_id);

        self.reply_response(src, "100 Trying", headers, None, None).await?;

        let (remote_ip, remote_port, ssrc, is_udp, media_rejected, passive) = parse_sdp(body)?;
        if media_rejected {
            self.reply_response(src, "200 OK", headers, None, Some(&self.local_tag)).await?;
            return Ok(());
        }

        let call_id = get_header(headers, "Call-ID").context("Call-ID missing")?;

        let app = &self.fixed_app;
        let stream = &self.fixed_stream_name;
        let media_list = self.zlm.as_ref().unwrap().get_media_list().await?;
        let stream_exists = media_list.iter().any(|m| {
            m.get("app").and_then(|v| v.as_str()) == Some(app)
                && m.get("stream").and_then(|v| v.as_str()) == Some(stream)
        });
        if !stream_exists {
            log::error!("Fixed stream {}:{} not found in ZLM", app, stream);
            self.reply_response(src, "503 Service Unavailable", headers, None, None).await?;
            return Ok(());
        }

        // 被动模式逻辑：上级要求被动 → 下级主动推流
        let local_port = if passive {
            log::debug!("上级要求被动，下级主动推流到 {}:{}", remote_ip, remote_port);
            self.zlm.as_ref().unwrap()
                .start_send_rtp(
                    "__defaultVhost__",
                    app,
                    stream,
                    ssrc,
                    &remote_ip,
                    remote_port,
                    is_udp,
                    None,
                    None,
                    None,
                )
                .await?
        } else {
            log::debug!("上级主动，下级被动等待连接");
            self.zlm.as_ref().unwrap()
                .start_send_rtp_passive("__defaultVhost__", app, stream, ssrc, is_udp, None, false)
                .await?
        };

        let info = SessionInfo {
            ssrc,
            stream_name: stream.clone(),
            zlm_api_base: self.zlm.as_ref().unwrap().api_base().to_string(),
            zlm_secret: self.zlm.as_ref().unwrap().secret().to_string(),
            app: app.clone(),
        };
        add_session(&self.sessions, call_id, info).await;

        let media_ip = &self.public_ip;
        let mut sdp = format!(
            "v=0\r\n\
             o={} 0 0 IN IP4 {}\r\n\
             s=Play\r\n\
             c=IN IP4 {}\r\n\
             t=0 0\r\n\
             m=video {} {} 96\r\n",
            self.config.device_id,
            media_ip,
            media_ip,
            local_port,
            if is_udp == 1 { "RTP/AVP" } else { "TCP/RTP/AVP" }
        );
        if is_udp == 0 {
            if passive {
                sdp += "a=setup:active\r\na=connection:new\r\n";
            } else {
                sdp += "a=setup:passive\r\na=connection:new\r\n";
            }
        }
        sdp += &format!("a=rtpmap:96 PS/90000\r\ny={}\r\n", ssrc);

        log::debug!("SDP response:\n{}", sdp);
        self.reply_response(src, "200 OK", headers, Some(&sdp), Some(&self.local_tag)).await?;
        Ok(())
    }

    pub async fn handle_bye(&self, src: SocketAddr, headers: &[String]) -> anyhow::Result<()> {
        let call_id = get_header(headers, "Call-ID").context("Call-ID missing")?;
        if let Some(info) = get_session(&self.sessions, call_id).await {
            let zlm = crate::zlm_client::ZlmClient::for_node(&info.zlm_api_base, &info.zlm_secret);
            let _ = zlm
                .stop_send_rtp(
                    "__defaultVhost__",
                    &info.app,
                    &info.stream_name,
                    Some(info.ssrc),
                )
                .await;
            remove_session(&self.sessions, call_id).await;
        }
        self.reply_response(src, "200 OK", headers, None, None).await?;
        Ok(())
    }
}