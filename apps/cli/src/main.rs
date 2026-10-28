//! byte-port CLI：命令行端口/进程管理。
//!
//! 主工作流：search（搜索定位）→ show（看详情）→ kill（关闭）。
//! orphans 检测/清理孤儿进程；ls 作为「列全部」兜底保留。

use std::io::{self, BufRead, Write};

use byte_port_core::filter::FilterOpts;
use byte_port_core::model::{PortEntry, Protocol};
use byte_port_core::proc_tree::{
    build_subtree, child_count, find_orphans, kill_tree, ProcNode,
};
use byte_port_core::query::find_by_pid;
use byte_port_core::{filter, scan_all, Pid, System};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "byte-port",
    version,
    about = "端口 / 进程管理命令行工具",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 搜索端口/进程（关键字同时匹配端口号、PID、进程名、路径、地址）
    Search(SearchArgs),
    /// 查看某进程的详情（占用端口、可执行路径）
    Show(ShowArgs),
    /// 结束指定 PID 的进程及其子进程（级联，执行前预览）
    Kill(KillArgs),
    /// 检测/清理孤儿进程（父进程已退出者）
    Orphans(OrphansArgs),
    /// 列出端口（不给关键字则列全部）
    Ls(LsArgs),
}

#[derive(clap::Args)]
struct SearchArgs {
    /// 关键字：同时匹配 端口号/PID/进程名/路径/地址（大小写不敏感）
    keyword: String,
    /// 仅 TCP 监听端口
    #[arg(long)]
    listen: bool,
    /// 按协议过滤：tcp / udp
    #[arg(long, value_name = "PROTO")]
    proto: Option<String>,
    /// 以 JSON 数组输出
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct ShowArgs {
    /// 目标进程 PID
    pid: u32,
    /// 以 JSON 输出
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct KillArgs {
    /// 目标进程 PID
    pid: u32,
    /// 跳过确认，直接结束（仍级联子进程）
    #[arg(short = 'f', long)]
    force: bool,
}

#[derive(clap::Args)]
struct OrphansArgs {
    /// 清理指定孤儿 PID（可多次指定），级联结束其子树
    #[arg(long, value_name = "PID", action = clap::ArgAction::Append)]
    kill: Vec<u32>,
    /// 清理全部孤儿（高危，二次确认）
    #[arg(long)]
    kill_all: bool,
    /// 以 JSON 输出
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct LsArgs {
    /// 可选关键字（给关键字则搜索，不给则列全部）
    keyword: Option<String>,
    /// 仅 TCP 监听端口
    #[arg(long)]
    listen: bool,
    /// 按协议过滤：tcp / udp
    #[arg(long, value_name = "PROTO")]
    proto: Option<String>,
    /// 按精确 PID 过滤
    #[arg(long, value_name = "PID")]
    pid: Option<u32>,
    /// 按进程名子串过滤（大小写不敏感）
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
    /// 以 JSON 数组输出
    #[arg(long)]
    json: bool,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Search(a) => cmd_search(a),
        Cmd::Show(a) => cmd_show(a),
        Cmd::Kill(a) => cmd_kill(a),
        Cmd::Orphans(a) => cmd_orphans(a),
        Cmd::Ls(a) => cmd_ls(a),
    }
}

fn parse_proto(s: &Option<String>) -> Option<Protocol> {
    s.as_ref().map(|p| match p.to_ascii_lowercase().as_str() {
        "tcp" => Protocol::Tcp,
        "udp" => Protocol::Udp,
        other => {
            eprintln!("无效的 --proto 值：{other}（应为 tcp 或 udp）");
            std::process::exit(2);
        }
    })
}

/// 搜索：关键字跨字段模糊匹配。
fn cmd_search(a: SearchArgs) {
    let ports = scan_all();
    let opts = FilterOpts {
        text: a.keyword,
        only_listen: a.listen,
        proto: parse_proto(&a.proto),
        ..Default::default()
    };
    let view: Vec<&PortEntry> = filter::filter_entries(&ports, &opts);
    if view.is_empty() {
        println!("未找到匹配");
        return;
    }
    if a.json {
        print_json(&view);
    } else {
        print_table(&view);
    }
}

/// 查看某进程详情：进程名 + exe 路径 + 占用端口列表。
fn cmd_show(a: ShowArgs) {
    let ports = scan_all();
    let entries = find_by_pid(&ports, a.pid);
    if entries.is_empty() {
        eprintln!("PID {} 未发现占用端口（进程可能不存在，或需管理员权限查看）", a.pid);
        std::process::exit(1);
    }
    // 进程信息从首条记录取（同 PID 的进程名/路径一致）
    let name = entries[0].process_name.clone().unwrap_or_default();
    let exe = entries[0]
        .exe_path
        .clone()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    if a.json {
        print_detail_json(a.pid, &name, &exe, &entries);
        return;
    }
    println!("PID      : {}", a.pid);
    println!("进程名   : {}", if name.is_empty() { "(未知)" } else { &name });
    println!("可执行   : {}", if exe.is_empty() { "(未知)" } else { &exe });
    println!("占用端口 : {}", entries.len());
    println!();
    print_table(&entries);
}

/// 结束进程：级联整棵子树，先预览再确认。
fn cmd_kill(a: KillArgs) {
    let sys = System::new_all();
    let root_pid = Pid::from_u32(a.pid);

    // 构建子树（级联所需）
    let tree = build_subtree(&sys, root_pid);
    let total = tree.total_count();

    // 预览
    println!("即将结束 PID {}（含子进程，共 {} 个进程）:", a.pid, total);
    print_tree_preview(&tree, 1);

    if total > 1 {
        println!("  ⚠ 将级联结束以上子进程");
    }

    if !a.force {
        if !confirm("确认结束?") {
            println!("已取消");
            return;
        }
    }

    // 后序 kill
    let results = kill_tree(&sys, &tree);
    let (ok, fail) = summarize(results);
    println!("完成：成功 {} / 失败 {}", ok, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}

/// 检测/清理孤儿进程。
fn cmd_orphans(a: OrphansArgs) {
    let sys = System::new_all();

    // 清理模式
    if a.kill_all {
        let orphans = find_orphans(&sys);
        if orphans.is_empty() {
            println!("无孤儿进程");
            return;
        }
        println!("发现 {} 个孤儿进程，即将全部清理：", orphans.len());
        for o in &orphans {
            println!("  PID {}  {}", o.pid.as_u32(), o.name);
        }
        if !confirm("确认清理全部孤儿?") {
            println!("已取消");
            return;
        }
        let mut ok = 0usize;
        let mut fail = 0usize;
        for o in &orphans {
            let tree = build_subtree(&sys, o.pid);
            let results = kill_tree(&sys, &tree);
            let (o_ok, o_fail) = summarize(results);
            ok += o_ok;
            fail += o_fail;
        }
        println!("完成：成功 {} / 失败 {}", ok, fail);
        return;
    }

    if !a.kill.is_empty() {
        let mut ok = 0usize;
        let mut fail = 0usize;
        for pid in &a.kill {
            let root_pid = Pid::from_u32(*pid);
            let tree = build_subtree(&sys, root_pid);
            println!("清理孤儿 PID {}（含子进程 {} 个）", pid, tree.total_count());
            let results = kill_tree(&sys, &tree);
            let (o_ok, o_fail) = summarize(results);
            ok += o_ok;
            fail += o_fail;
        }
        println!("完成：成功 {} / 失败 {}", ok, fail);
        return;
    }

    // 列出模式
    let orphans = find_orphans(&sys);
    if orphans.is_empty() {
        println!("无孤儿进程");
        return;
    }
    if a.json {
        print_orphans_json(&orphans, &sys);
        return;
    }
    print_orphans_table(&orphans, &sys);
}

// ---------- 孤儿/进程树输出 ----------

fn print_orphans_table(orphans: &[ProcNode], sys: &System) {
    let rows: Vec<[String; 4]> = orphans
        .iter()
        .map(|o| {
            let exe = o
                .exe
                .clone()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            [
                o.pid.as_u32().to_string(),
                o.name.clone(),
                exe,
                child_count(sys, o.pid).to_string(),
            ]
        })
        .collect();
    let headers = ["PID", "进程名", "路径", "子进程数"];
    let widths = compute_widths4(&headers, &rows);

    print_row(&headers.map(|s| s.to_string()), &widths);
    print_sep(&widths);
    for r in &rows {
        print_row(r, &widths);
    }
    println!("\n共 {} 个孤儿进程", orphans.len());
}

fn print_orphans_json(orphans: &[ProcNode], sys: &System) {
    println!("[");
    for (idx, o) in orphans.iter().enumerate() {
        let exe = o
            .exe
            .clone()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("  {{");
        println!("    \"pid\": {},", o.pid.as_u32());
        println!("    \"name\": \"{}\",", json_escape(&o.name));
        println!("    \"exe\": \"{}\",", json_escape(&exe));
        println!("    \"children\": {}", child_count(sys, o.pid));
        let comma = if idx + 1 < orphans.len() { "," } else { "" };
        println!("  }}{}", comma);
    }
    println!("]");
}

/// 树形预览：缩进展示 PID + 进程名。
fn print_tree_preview(node: &ProcNode, depth: usize) {
    let indent = "  ".repeat(depth);
    println!("{}├ PID {}  {}", indent, node.pid.as_u32(), node.name);
    for c in &node.children {
        print_tree_preview(c, depth + 1);
    }
}

// ---------- 通用辅助 ----------

/// 读取一行，y/Y 返回 true，其余 false。
fn confirm(prompt: &str) -> bool {
    print!("{} [y/N] ", prompt);
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    line.trim().eq_ignore_ascii_case("y")
}

/// 统计 kill 结果的成功/失败数。
fn summarize(results: Vec<(Pid, bool)>) -> (usize, usize) {
    let ok = results.iter().filter(|(_, b)| *b).count();
    let fail = results.iter().filter(|(_, b)| !*b).count();
    (ok, fail)
}

fn compute_widths4(headers: &[&str; 4], rows: &[[String; 4]]) -> [usize; 4] {
    let mut w = [0usize; 4];
    for (i, h) in headers.iter().enumerate() {
        w[i] = h.chars().count().max(w[i]);
    }
    for r in rows {
        for (i, cell) in r.iter().enumerate() {
            w[i] = cell.chars().count().max(w[i]);
        }
    }
    w
}

fn print_row(cells: &[String], widths: &[usize]) {
    let mut line = String::new();
    for (i, cell) in cells.iter().enumerate() {
        line.push_str(&pad(cell, widths[i]));
        line.push(' ');
    }
    println!("{line}");
}

fn print_sep(widths: &[usize]) {
    let mut sep = String::new();
    for &w in widths {
        sep.push_str(&"-".repeat(w));
        sep.push(' ');
    }
    println!("{sep}");
}

/// 列端口：有关键字则搜索，无则列全部。
fn cmd_ls(a: LsArgs) {
    let ports = scan_all();
    let opts = FilterOpts {
        text: a.keyword.unwrap_or_default(),
        only_listen: a.listen,
        proto: parse_proto(&a.proto),
        pid: a.pid,
        name: a.name,
    };
    let view: Vec<&PortEntry> = filter::filter_entries(&ports, &opts);
    if a.json {
        print_json(&view);
        return;
    }
    if view.is_empty() {
        println!("未找到匹配");
        return;
    }
    print_table(&view);
}

// ---------- 输出格式化 ----------

fn print_table(view: &[&PortEntry]) {
    let rows: Vec<[String; 6]> = view
        .iter()
        .map(|e| {
            [
                match e.protocol {
                    Protocol::Tcp => "TCP".into(),
                    Protocol::Udp => "UDP".into(),
                },
                e.local_addr.clone(),
                e.state.clone(),
                e.pid.map(|p| p.to_string()).unwrap_or_default(),
                e.process_name.clone().unwrap_or_default(),
                e.exe_path
                    .clone()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ]
        })
        .collect();

    let headers = ["协议", "本地地址:端口", "状态", "PID", "进程名", "路径"];
    let widths = compute_widths(&headers, &rows);

    let mut h = String::new();
    for (i, name) in headers.iter().enumerate() {
        h.push_str(&pad(name, widths[i]));
        h.push(' ');
    }
    println!("{h}");
    let mut sep = String::new();
    for &w in widths.iter() {
        sep.push_str(&"-".repeat(w));
        sep.push(' ');
    }
    println!("{sep}");
    for r in &rows {
        let mut line = String::new();
        for (i, cell) in r.iter().enumerate() {
            line.push_str(&pad(cell, widths[i]));
            line.push(' ');
        }
        println!("{line}");
    }
    println!("\n共 {} 条", view.len());
}

fn compute_widths(headers: &[&str; 6], rows: &[[String; 6]]) -> [usize; 6] {
    let mut w = [0usize; 6];
    for (i, h) in headers.iter().enumerate() {
        w[i] = h.chars().count().max(w[i]);
    }
    for r in rows {
        for (i, cell) in r.iter().enumerate() {
            w[i] = cell.chars().count().max(w[i]);
        }
    }
    w
}

fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

/// 端口列表 JSON 数组（带中日韩字符安全转义）。
fn print_json(view: &[&PortEntry]) {
    println!("[");
    for (idx, e) in view.iter().enumerate() {
        let proto = match e.protocol {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        };
        println!("  {{");
        println!("    \"proto\": \"{}\",", proto);
        println!("    \"local\": \"{}\",", e.local_addr);
        println!("    \"state\": \"{}\",", e.state);
        println!("    \"pid\": {},", e.pid.unwrap_or(0));
        println!("    \"name\": \"{}\",", json_escape(&e.process_name.clone().unwrap_or_default()));
        println!("    \"exe\": \"{}\"", json_escape(&e.exe_path.clone().map(|p| p.display().to_string()).unwrap_or_default()));
        let comma = if idx + 1 < view.len() { "," } else { "" };
        println!("  }}{}", comma);
    }
    println!("]");
}

/// 进程详情 JSON（进程信息 + 端口数组）。
fn print_detail_json(pid: u32, name: &str, exe: &str, entries: &[&PortEntry]) {
    println!("{{");
    println!("  \"pid\": {},", pid);
    println!("  \"name\": \"{}\",", json_escape(name));
    println!("  \"exe\": \"{}\",", json_escape(exe));
    println!("  \"ports\": [");
    for (idx, e) in entries.iter().enumerate() {
        let proto = match e.protocol {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        };
        println!("    {{");
        println!("      \"proto\": \"{}\",", proto);
        println!("      \"local\": \"{}\",", e.local_addr);
        println!("      \"state\": \"{}\"", e.state);
        let comma = if idx + 1 < entries.len() { "," } else { "" };
        println!("    }}{}", comma);
    }
    println!("  ]");
    println!("}}");
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
