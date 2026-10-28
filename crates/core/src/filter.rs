//! 端口条目筛选。GUI 与 CLI 共用同一套过滤规则，避免重复实现。

use crate::model::{PortEntry, Protocol};

/// 筛选参数。`text` 为空时不按关键字过滤。
#[derive(Debug, Clone, Default)]
pub struct FilterOpts {
    /// 模糊关键字（对协议/地址/状态/PID/进程名/路径做 lowercase contains）
    pub text: String,
    /// 仅 TCP LISTEN
    pub only_listen: bool,
    /// 按协议过滤
    pub proto: Option<Protocol>,
    /// 按精确 PID 过滤
    pub pid: Option<u32>,
    /// 按进程名子串过滤（大小写不敏感）
    pub name: Option<String>,
}

impl FilterOpts {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 按给定条件筛选端口条目，返回引用视图（不克隆数据）。
///
/// 与原 GUI 内联逻辑等价：先过 `only_listen` / `proto` / `pid` / `name`，
/// 再对全字段做 `text` 模糊匹配。结果保持输入顺序（调用方按需排序）。
pub fn filter_entries<'a>(entries: &'a [PortEntry], opts: &FilterOpts) -> Vec<&'a PortEntry> {
    let needle = opts.text.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            // 仅监听：仅作用于 TCP 的 LISTEN 状态
            if opts.only_listen && !(e.protocol == Protocol::Tcp && e.state == "LISTEN") {
                return false;
            }
            // 协议
            if let Some(p) = &opts.proto {
                if e.protocol != *p {
                    return false;
                }
            }
            // 精确 PID
            if let Some(pid) = opts.pid {
                if e.pid != Some(pid) {
                    return false;
                }
            }
            // 进程名子串
            if let Some(name) = &opts.name {
                let nm = name.to_lowercase();
                let hit = e
                    .process_name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&nm))
                    .unwrap_or(false);
                if !hit {
                    return false;
                }
            }
            // 全字段模糊匹配
            if needle.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {} {} {} {}",
                match e.protocol {
                    Protocol::Tcp => "tcp",
                    Protocol::Udp => "udp",
                },
                e.local_addr,
                e.state,
                e.pid.map(|p| p.to_string()).unwrap_or_default(),
                e.process_name.clone().unwrap_or_default(),
                e.exe_path
                    .clone()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            );
            hay.to_lowercase().contains(&needle)
        })
        .collect()
}
