//! 进程树与孤儿进程检测。
//!
//! ## 概念
//! - **孤儿 orphan**：父进程已退出（其 PID 不在存活集合中），子进程仍在运行。
//!   跨平台都有，但 Linux/macOS 由 init/launchd(PID1) 自动收养，视为正常机制；
//!   Windows 无收养机制，父进程退出后子进程真正孤立、需手动清理。
//! - **级联 kill**：结束某 PID 时，先递归结束其所有子孙，再结束自身，
//!   避免制造新的孤儿。
//!
//! 所有函数接收外部 `&System` 快照，保证一致性、避免重复扫描。

use std::collections::HashSet;
use std::path::PathBuf;

use sysinfo::{Pid, System};

/// 进程树节点。
#[derive(Debug, Clone)]
pub struct ProcNode {
    pub pid: Pid,
    pub name: String,
    pub exe: Option<PathBuf>,
    /// 直接子进程（递归构建时填充）。
    pub children: Vec<ProcNode>,
}

impl ProcNode {
    /// 以本节点为根的子树节点总数（含自身）。
    pub fn total_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.total_count()).sum::<usize>()
    }
}

/// 当前存活 PID 集合。
pub fn alive_pids(sys: &System) -> HashSet<Pid> {
    sys.processes().keys().copied().collect()
}

/// 判断给定 PID 是否为孤儿：父进程不存在（None）或父 PID 不在存活集合中。
///
/// 系统关键进程 PID 0 / 1 永不视为孤儿（System Idle / init / launchd 等）。
pub fn is_orphan(sys: &System, pid: Pid) -> bool {
    // 系统关键进程排除
    let raw = pid.as_u32();
    if raw <= 1 {
        return false;
    }
    let Some(proc_info) = sys.process(pid) else {
        return false; // 自身已不存在，谈不上孤儿
    };
    match proc_info.parent() {
        None => true,
        Some(parent_pid) => {
            let parent_raw = parent_pid.as_u32();
            // 父是 PID 0/1：视为已被 init 收养，不算孤儿
            if parent_raw <= 1 {
                return false;
            }
            // 父 PID 不在存活集合 → 真正的孤儿
            !sys.process(parent_pid).is_some()
        }
    }
}

/// 收集系统中所有孤儿进程（轻量，只取 pid/name/exe，不递归子树）。
pub fn find_orphans(sys: &System) -> Vec<ProcNode> {
    sys.processes()
        .iter()
        .filter(|(pid, _)| is_orphan(sys, **pid))
        .map(|(pid, p)| ProcNode {
            pid: *pid,
            name: p.name().to_string_lossy().into_owned(),
            exe: p.exe().map(|x| x.to_path_buf()),
            children: Vec::new(),
        })
        .collect()
}

/// 统计某 PID 的直接子进程数量（用于孤儿列表展示）。
pub fn child_count(sys: &System, pid: Pid) -> usize {
    sys.processes()
        .values()
        .filter(|p| p.parent() == Some(pid))
        .count()
}

/// 构建以 `root_pid` 为根的子树（DFS，含所有递归子孙）。
///
/// 若 root_pid 不存在，返回一个仅含 pid 的占位节点。
pub fn build_subtree(sys: &System, root_pid: Pid) -> ProcNode {
    let (name, exe) = match sys.process(root_pid) {
        Some(p) => (
            p.name().to_string_lossy().into_owned(),
            p.exe().map(|x| x.to_path_buf()),
        ),
        None => (String::new(), None),
    };
    // 找直接子进程
    let children: Vec<ProcNode> = sys
        .processes()
        .iter()
        .filter(|(_, p)| p.parent() == Some(root_pid))
        .map(|(pid, _)| build_subtree(sys, *pid))
        .collect();
    ProcNode {
        pid: root_pid,
        name,
        exe,
        children,
    }
}

/// 后序遍历结束整棵子树：先结束所有子进程，再结束根。
///
/// 返回每个 PID 的结束结果 `(pid, success)`，失败不中断、继续兄弟节点。
/// 注意：进程快照可能已过时，kill 前用 sys.process 实时取句柄。
pub fn kill_tree(sys: &System, node: &ProcNode) -> Vec<(Pid, bool)> {
    let mut results = Vec::new();
    // 后序：先子后父
    for child in &node.children {
        results.extend(kill_tree(sys, child));
    }
    let ok = match sys.process(node.pid) {
        Some(p) => p.kill(),
        None => false, // 进程已不存在，视为失败（调用方可据此判断）
    };
    results.push((node.pid, ok));
    results
}
