//! 扫描快照与后台扫描封装。
//!
//! 扫描（iphlpapi 枚举 + sysinfo 进程表）是重 IO，放 background_spawn
//! 线程池执行；Snapshot 是纯数据（Send），回 UI 线程后存入缓存。

use byte_port_core::filter::{filter_entries, FilterOpts};
use byte_port_core::model::{PortEntry, Protocol};
use byte_port_core::proc_tree::{child_count, find_orphans, ProcNode};
use byte_port_core::{scan_all, System};

/// 一次扫描产出的完整快照。
pub struct Snapshot {
    pub ports: Vec<PortEntry>,
    pub orphans: Vec<ProcNode>,
    pub sys: System,
}

/// 执行一次完整扫描（在后台线程池调用；要求返回值 Send，Snapshot 满足）。
pub fn scan_snapshot() -> Snapshot {
    let sys = System::new_all();
    let orphans = find_orphans(&sys);
    let ports = scan_all();
    Snapshot { ports, orphans, sys }
}

/// 按关键字过滤端口条目（返回克隆，供表格 delegate 使用）。
pub fn filter_ports(snap: &Snapshot, text: &str) -> Vec<PortEntry> {
    let opts = FilterOpts {
        text: text.to_string(),
        ..Default::default()
    };
    filter_entries(&snap.ports, &opts).into_iter().cloned().collect()
}

/// 按关键字过滤孤儿（匹配 PID/进程名/路径）。
pub fn filter_orphans(snap: &Snapshot, text: &str) -> Vec<ProcNode> {
    let needle = text.to_lowercase();
    snap.orphans
        .iter()
        .filter(|o| {
            if needle.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {}",
                o.pid.as_u32(),
                o.name,
                o.exe.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
            );
            hay.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

/// 子树扁平化（depth + 缩进标签），供弹窗预览。
pub fn flatten_tree(node: &ProcNode, depth: usize, out: &mut Vec<(String, usize)>) {
    out.push((format!("PID {}  {}", node.pid.as_u32(), node.name), depth));
    for c in &node.children {
        flatten_tree(c, depth + 1, out);
    }
}

/// 协议显示名。
pub fn proto_label(p: &Protocol) -> &'static str {
    match p {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
    }
}

/// 孤儿子进程数（需要 sys 快照）。
pub fn orphan_children(sys: &System, pid: &byte_port_core::Pid) -> usize {
    child_count(sys, *pid)
}
