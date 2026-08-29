use encoding_rs::{GB18030, GBK};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const LINESEP: &str = "\r\n";

pub fn xml_header(version: &str) -> String {
    if version == "2022" || version == "3.0" {
        "<?xml version=\"1.0\" encoding=\"GB18030\"?>\r\n".to_string()
    } else {
        "<?xml version=\"1.0\" encoding=\"GB2312\"?>\r\n".to_string()
    }
}

pub fn encode_xml(version: &str, xml: &str) -> Vec<u8> {
    if version == "2022" || version == "3.0" {
        let (bytes, _, _) = GB18030.encode(xml);
        bytes.into_owned()
    } else {
        let (bytes, _, _) = GBK.encode(xml);
        bytes.into_owned()
    }
}

pub fn gb_ver(version: &str) -> &'static str {
    if version == "2022" || version == "3.0" {
        "3.0"
    } else {
        "2.0"
    }
}

pub fn md5_hash(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

pub fn sha256_hash(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn parse_sip_lines(data: &str) -> anyhow::Result<(String, Vec<String>)> {
    let mut lines = data.split(LINESEP);
    let first = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty message"))?
        .to_string();
    let headers: Vec<String> = lines
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    Ok((first, headers))
}

pub fn get_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    let lower_name = name.to_lowercase();
    headers
        .iter()
        .find(|h| h.to_lowercase().starts_with(&lower_name))
        .and_then(|h| h.split_once(':'))
        .map(|(_, v)| v.trim())
}

pub fn extract_xml_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);
    let start = xml.find(&start_tag)? + start_tag.len();
    let end = xml[start..].find(&end_tag)?;
    let raw = &xml[start..start + end];
    let cleaned = raw
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .trim();
    Some(cleaned)
}

pub fn parse_www_authenticate(header: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let header = header.trim_start_matches("Digest ");
    for part in header.split(',') {
        let part = part.trim();
        if let Some(eq) = part.find('=') {
            let key = part[..eq].trim().to_lowercase();
            let value = part[eq + 1..].trim().trim_matches('"');
            params.insert(key, value.to_string());
        }
    }
    params
}

pub fn parse_sdp(body: &str) -> anyhow::Result<(String, u16, u32, u8, bool, bool)> {
    let lines: Vec<&str> = body.lines().collect();
    let conn = lines
        .iter()
        .find(|l| l.starts_with("c="))
        .ok_or_else(|| anyhow::anyhow!("no c="))?;
    let remote_ip = conn
        .split_whitespace()
        .last()
        .ok_or_else(|| anyhow::anyhow!("no ip"))?
        .to_string();
    let media = lines
        .iter()
        .find(|l| l.starts_with("m="))
        .ok_or_else(|| anyhow::anyhow!("no m="))?;
    let parts: Vec<&str> = media.split_whitespace().collect();
    if parts.len() < 4 {
        anyhow::bail!("invalid m= line: {}", media);
    }
    let media_port: i32 = parts[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid media port"))?;
    if media_port == -1 {
        return Ok((String::new(), 0, 0, 0, true, false));
    }
    if media_port < 0 || media_port > 65535 {
        anyhow::bail!("invalid media port value: {}", media_port);
    }
    let is_udp = if parts[2] == "TCP/RTP/AVP" { 0 } else { 1 };
    let ssrc = lines
        .iter()
        .find(|l| l.starts_with("y="))
        .and_then(|l| l.trim_start_matches("y=").parse::<u32>().ok())
        .unwrap_or(0);
    let passive = lines.iter().any(|l| l.trim() == "a=setup:passive");
    Ok((remote_ip, media_port as u16, ssrc, is_udp, false, passive))
}
