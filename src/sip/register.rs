use crate::sip::state::SipState;
use crate::sip::utils::*;
use anyhow::Context;
use std::net::SocketAddr;

impl SipState {
    pub async fn send_register_to(
        &self,
        dest: SocketAddr,
        expires: u32,
        with_auth: bool,
    ) -> anyhow::Result<Option<u64>> {
        let version = self.peer_version.lock().await.clone();
        let mut cseq = self.cseq.lock().await;
        let seq = *cseq;
        *cseq += 1;
        drop(cseq);

        let branch = format!("z9hG4bK-{}", hex::encode(&rand::random::<[u8; 4]>()));
        let contact_ip = if self.public_ip.is_empty() {
            self.config.local_addr.ip().to_string()
        } else {
            self.public_ip.clone()
        };
        let local_port = self.config.local_addr.port();
        let transport = self.via_transport();
        let realm = &self.config.realm;
        let device_id = &self.config.device_id;
        let upper_id = &self.upper_server_id;
        let call_id = &self.call_id_base;
        let local_tag = &self.local_tag;
        let via_ip = self.config.local_addr.ip();

        let base_msg = format!(
            "REGISTER sip:{}@{} SIP/2.0\r\n\
             Via: SIP/2.0/{} {}:{};rport;branch={}\r\n\
             From: <sip:{}@{}>;tag={}\r\n\
             To: <sip:{}@{}>\r\n\
             Call-ID: {}\r\n\
             CSeq: {} REGISTER\r\n\
             X-GB-Ver: {}\r\n\
             Contact: <sip:{}@{}:{}>\r\n\
             Max-Forwards: 70\r\n\
             User-Agent: GBHub-Stress\r\n\
             Expires: {}\r\n\
             Content-Length: 0\r\n\r\n",
            upper_id,
            dest,
            transport,
            via_ip,
            local_port,
            branch,
            device_id,
            realm,
            local_tag,
            upper_id,
            dest,
            call_id,
            seq,
            gb_ver(&version),
            device_id,
            contact_ip,
            local_port,
            expires
        );

        let full_msg = if with_auth {
            let nonce_guard = self.nonce.lock().await;
            let www_auth = nonce_guard
                .as_ref()
                .context("no WWW-Authenticate info available, cannot authenticate")?;
            let params = parse_www_authenticate(www_auth);
            let nonce = params
                .get("nonce")
                .context("no nonce in WWW-Authenticate")?;
            let algorithm = params
                .get("algorithm")
                .map(|a| a.to_uppercase())
                .unwrap_or_else(|| "MD5".to_string());
            let qop = params.get("qop").cloned();

            let ha1 = match algorithm.as_str() {
                "SHA256" | "SHA-256" => {
                    sha256_hash(&format!("{}:{}:{}", device_id, realm, self.config.password))
                }
                _ => md5_hash(&format!("{}:{}:{}", device_id, realm, self.config.password)),
            };

            let uri = format!("sip:{}@{}", upper_id, dest);
            let ha2 = match algorithm.as_str() {
                "SHA256" | "SHA-256" => sha256_hash(&format!("REGISTER:{}", uri)),
                _ => md5_hash(&format!("REGISTER:{}", uri)),
            };

            let (cnonce, nc, response, qop_str) = if qop.as_deref() == Some("auth") {
                let cnonce = hex::encode(&rand::random::<[u8; 4]>());
                let nc = "00000001";
                let response = match algorithm.as_str() {
                    "SHA256" | "SHA-256" => {
                        sha256_hash(&format!("{}:{}:{}:{}:auth:{}", ha1, nonce, nc, cnonce, ha2))
                    }
                    _ => md5_hash(&format!("{}:{}:{}:{}:auth:{}", ha1, nonce, nc, cnonce, ha2)),
                };
                (Some(cnonce), Some(nc), response, Some("auth"))
            } else {
                let response = match algorithm.as_str() {
                    "SHA256" | "SHA-256" => sha256_hash(&format!("{}:{}:{}", ha1, nonce, ha2)),
                    _ => md5_hash(&format!("{}:{}:{}", ha1, nonce, ha2)),
                };
                (None, None, response, None)
            };

            let mut auth_header = format!(
                "Authorization: Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\", algorithm={}",
                device_id,
                realm,
                nonce,
                uri,
                response,
                if algorithm == "SHA256" || algorithm == "SHA-256" { "SHA-256" } else { "MD5" }
            );
            if let Some(cn) = cnonce {
                auth_header += &format!(", cnonce=\"{}\"", cn);
            }
            if let Some(n) = nc {
                auth_header += &format!(", nc={}", n);
            }
            if let Some(q) = qop_str {
                auth_header += &format!(", qop={}", q);
            }
            auth_header += "\r\n";

            format!(
                "REGISTER sip:{}@{} SIP/2.0\r\n\
                 Via: SIP/2.0/{} {}:{};rport;branch={}\r\n\
                 From: <sip:{}@{}>;tag={}\r\n\
                 To: <sip:{}@{}>\r\n\
                 Call-ID: {}\r\n\
                 CSeq: {} REGISTER\r\n\
                 X-GB-Ver: {}\r\n\
                 Contact: <sip:{}@{}:{}>\r\n\
                 {}\
                 Max-Forwards: 70\r\n\
                 User-Agent: GBHub-Stress\r\n\
                 Expires: {}\r\n\
                 Content-Length: 0\r\n\r\n",
                upper_id,
                dest,
                transport,
                via_ip,
                local_port,
                branch,
                device_id,
                realm,
                local_tag,
                upper_id,
                dest,
                call_id,
                seq,
                gb_ver(&version),
                device_id,
                contact_ip,
                local_port,
                auth_header,
                expires
            )
        } else {
            base_msg
        };

        if self.config.transport.eq_ignore_ascii_case("TCP") {
            anyhow::bail!("TCP transport not supported in GBHub-Stress");
        } else {
            self.send_to(dest, &full_msg).await?;
            Ok(None)
        }
    }

    pub async fn register_with_retry(&self) -> anyhow::Result<()> {
        match self
            .send_register_to(self.config.sip_server, self.register_expires, false)
            .await
        {
            Ok(_) => Ok(()), // 不再调用 stats.mark_registered
            Err(e) => {
                if e.to_string().contains("401") || e.to_string().contains("407") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    match self
                        .send_register_to(self.config.sip_server, self.register_expires, true)
                        .await
                    {
                        Ok(_) => Ok(()), // 不再调用 stats.mark_registered
                        Err(e2) => {
                            log::error!("Authenticated REGISTER failed: {}", e2);
                            Err(e2)
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        let version = self.peer_version.lock().await.clone();
        let xml = format!(
            "{}<Notify>\r\n<CmdType>Keepalive</CmdType>\r\n<SN>1</SN>\r\n<DeviceID>{}</DeviceID>\r\n<Status>OK</Status>\r\n</Notify>\r\n",
            xml_header(&version),
            self.config.device_id
        );
        self.send_message_to(self.config.sip_server, &xml).await
    }

    pub async fn send_message_to(&self, dest: SocketAddr, xml: &str) -> anyhow::Result<()> {
        let version = self.peer_version.lock().await.clone();
        let mut cseq = self.cseq.lock().await;
        let seq = *cseq;
        *cseq += 1;
        drop(cseq);
        let encoded_body = encode_xml(&version, xml);
        let content_length = encoded_body.len();
        let branch = format!("z9hG4bK-{}", hex::encode(&rand::random::<[u8; 4]>()));
        let contact_ip = &self.public_ip;
        let local_port = self.config.local_addr.port();

        let header = format!(
            "MESSAGE sip:{}@{} SIP/2.0\r\n\
             Via: SIP/2.0/{} {}:{};rport;branch={}\r\n\
             From: <sip:{}@{}>;tag={}\r\n\
             To: <sip:{}@{}>\r\n\
             Call-ID: {}\r\n\
             CSeq: {} MESSAGE\r\n\
             X-GB-Ver: {}\r\n\
             Content-Type: application/MANSCDP+xml\r\n\
             Max-Forwards: 70\r\n\
             User-Agent: GBHub-Stress\r\n\
             Content-Length: {}\r\n\r\n",
            self.upper_server_id,
            self.config.realm,
            self.via_transport(),
            contact_ip,
            local_port,
            branch,
            self.config.device_id,
            self.config.realm,
            self.local_tag,
            self.upper_server_id,
            self.config.realm,
            self.call_id_base,
            seq,
            gb_ver(&version),
            content_length
        );

        let mut packet = header.into_bytes();
        packet.extend_from_slice(&encoded_body);
        if let Some(sock) = &self.socket {
            sock.send_to(&packet, dest).await?;
        }
        Ok(())
    }
}
