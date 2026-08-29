use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 压测统计信息
pub struct Stats {
    /// 已注册设备集合 (device_id → true)
    registered: Arc<DashMap<String, bool>>,
    /// 每个设备收到的 INVITE 次数 (device_id → count)
    invite_counts: Arc<DashMap<String, u64>>,
    /// 总 INVITE 次数（原子计数器，快速统计）
    total_invites: Arc<AtomicU64>,
    /// 设备总数（用于计算注册率）
    total_devices: u64,
}

impl Stats {
    /// 创建新的统计实例
    pub fn new(total_devices: u64) -> Self {
        Self {
            registered: Arc::new(DashMap::new()),
            invite_counts: Arc::new(DashMap::new()),
            total_invites: Arc::new(AtomicU64::new(0)),
            total_devices,
        }
    }

    /// 标记设备已注册
    pub fn mark_registered(&self, device_id: &str) {
        self.registered.insert(device_id.to_string(), true);
    }

    /// 记录一次 INVITE 请求
    pub fn mark_invite(&self, device_id: &str) {
        // 原子操作增加总计数
        self.total_invites.fetch_add(1, Ordering::Relaxed);

        // 按设备累计
        let key = device_id.to_string();
        let mut entry = self.invite_counts.entry(key).or_insert(0);
        *entry += 1;
    }

    /// 获取已注册设备数量
    pub fn registered_count(&self) -> usize {
        self.registered.len()
    }

    /// 获取总 INVITE 次数
    pub fn total_invites(&self) -> u64 {
        self.total_invites.load(Ordering::Relaxed)
    }

    /// 获取涉及 INVITE 的设备数量
    pub fn invited_devices_count(&self) -> usize {
        self.invite_counts.len()
    }

    /// 打印统计信息
    pub async fn print_stats(&self) {
        let registered = self.registered_count();
        let total = self.total_devices;
        let rate = if total > 0 {
            (registered as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        let invites = self.total_invites();
        let invited_devices = self.invited_devices_count();

        println!(
            "\n========== GBHub-Stress 压测统计 =========="
        );
        println!("  已注册设备数: {}/{} ({:.2}%)", registered, total, rate);
        println!("  总 INVITE 请求数: {}", invites);
        println!("  涉及设备数: {}", invited_devices);
        println!("==========================================\n");
    }

    /// 启动定时打印（每 30 秒）
    pub fn start_periodic_print(self: Arc<Self>, interval_secs: u64) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                self.print_stats().await;
            }
        });
    }
}

// 方便克隆 Stats 实例
impl Clone for Stats {
    fn clone(&self) -> Self {
        Self {
            registered: self.registered.clone(),
            invite_counts: self.invite_counts.clone(),
            total_invites: self.total_invites.clone(),
            total_devices: self.total_devices,
        }
    }
}