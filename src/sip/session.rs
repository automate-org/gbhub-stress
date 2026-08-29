use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub ssrc: u32,
    pub stream_name: String,
    pub zlm_api_base: String,
    pub zlm_secret: String,
    pub app: String,
}

pub type SessionStore = Arc<DashMap<String, SessionInfo>>;

pub async fn add_session(store: &SessionStore, call_id: &str, info: SessionInfo) {
    store.insert(call_id.to_string(), info);
}

pub async fn get_session(store: &SessionStore, call_id: &str) -> Option<SessionInfo> {
    store.get(call_id).map(|v| v.clone())
}

pub async fn remove_session(store: &SessionStore, call_id: &str) {
    store.remove(call_id);
}