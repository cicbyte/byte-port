//! byte-port 核心库：端口扫描、进程关联、筛选、查询与进程树。
//!
//! GUI 与 CLI 共用本 crate，系统调用层与 UI 框架解耦。

pub mod filter;
pub mod model;
pub mod port_scanner;
pub mod proc_tree;
pub mod query;

// 常用项重导出，方便调用方 `use byte_kill_core::{...}`
pub use model::{PortEntry, Protocol};
pub use port_scanner::{kill_pid, scan_all};
pub use proc_tree::{find_orphans, kill_tree, ProcNode};
// 重导出 sysinfo 类型，避免调用方再直接依赖 sysinfo（统一版本）
pub use sysinfo::{Pid, System};
