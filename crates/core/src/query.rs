//! 端口查询：按端口号 / PID 定位条目，供 CLI `search`/`show`/`which` 子命令使用。

use crate::model::PortEntry;

/// 找出所有占用指定本地端口的条目（TCP/UDP 均查，一个端口可能同时被多协议/多进程占用）。
pub fn find_by_port<'a>(entries: &'a [PortEntry], port: u16) -> Vec<&'a PortEntry> {
    entries.iter().filter(|e| e.local_port == port).collect()
}

/// 找出指定 PID 进程占用的全部端口条目（一个进程常占用多个 TCP/UDP 端口）。
pub fn find_by_pid<'a>(entries: &'a [PortEntry], pid: u32) -> Vec<&'a PortEntry> {
    entries.iter().filter(|e| e.pid == Some(pid)).collect()
}
