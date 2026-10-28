# byte-port

端口 / 进程管理工具，Rust 实现，零 web、原生二进制、GPU 渲染。
提供 **GUI**（gpui-kit）与 **CLI** 两种前端，共用同一核心库。

## 仓库结构

Cargo workspace 多 crate：

```
byte-port/
├── crates/
│   └── core/          # byte-port-core：扫描 / 进程关联 / 筛选 / 查询 / 进程树
└── apps/
    ├── gui/           # byte-port-gui：gpui-kit 桌面前端（GPUI 渲染 + Catppuccin Mocha）
    │   └── themes/    # catppuccin.json（内嵌进二进制）
    └── cli/           # byte-port-cli：命令行前端
```

- `byte-port-core`：端口枚举（Windows `iphlpapi` 直读 FFI）、`sysinfo` 进程关联、
  共享的筛选（`filter`）与端口查询（`query`）。系统调用层与 UI 完全解耦。
- `byte-port-gui`：即时模式 GUI，后台线程扫描 → `mpsc` 回传，表格 + 暗色主题。
- `byte-port-cli`：`clap` 子命令式 CLI。

## 功能

### GUI（gpui-kit，Catppuccin Mocha 深色主题）
百度式双态界面，启动不预扫、输入回车/点搜索才扫描。
- **首页态**：居中大搜索框 + 类型选择（端口/孤儿）
- **结果态**：顶部小搜索框 + TabBar + DataTable 结果表
- **端口页**：扫描全部 TCP/UDP 端口，端口→PID→进程名/路径关联；行内「结束」，
  弹窗预览整棵子进程树后级联结束
- **孤儿页**：列出父进程已退出的孤儿（PID/进程名/路径/子进程数）；
  逐个「清理」或「全部清理」，均带二次确认
- 切 tab 不重扫，各自独立缓存；中文渲染零配置（Windows DirectWrite 系统字体回退）

### CLI
主工作流：**search 定位 → show 看详情 → kill 关闭**；另支持 **orphans 孤儿进程清理**。
- `search <关键字>`：跨字段搜索（端口号/PID/进程名/路径/地址），支持 `--listen` /
  `--proto <tcp|udp>` / `--json`
- `show <pid>`：查看某进程占用的全部端口 + 可执行路径，`--json` 机器可读
- `kill <pid>`：结束进程及其子进程（级联整棵子树，执行前预览），`-f / --force` 跳过确认
- `orphans`：检测/清理孤儿进程（父进程已退出者）。默认列出；`--kill <pid>` 清理指定、
  `--kill-all` 清理全部（高危，二次确认）、`--json` 机器可读
- `ls [关键字]`：列出端口，有关键字则搜索、无则列全部；支持 `--listen` / `--proto` /
  `--pid <N>` / `--name <str>` / `--json`

#### 关于孤儿进程
孤儿进程（父进程已退出、子进程仍在运行）**不是 Windows 独有问题**，三大平台都有。
差异在于：Linux/macOS 由 `init`/`launchd`(PID 1) **自动收养**，被视为正常机制；
**Windows 无收养机制**，父进程退出后子进程真正孤立，所以最容易残留、最需要手动清理。
本工具的检测逻辑基于 `sysinfo::Process::parent()`，跨平台通用，但对 Windows 价值最大。
系统关键进程（PID 0/1）永不视为孤儿；所有 kill 默认二次确认。

## 构建与运行

```bash
# 编译整个工作区
cargo build --workspace

# --- CLI ---
cargo run -p byte-port-cli -- search 8080          # 搜索端口/进程
cargo run -p byte-port-cli -- search svchost --listen
cargo run -p byte-port-cli -- show 1588            # 看某进程详情
cargo run -p byte-port-cli -- kill 1234 -f         # 关闭进程及子进程（-f 跳过确认）
cargo run -p byte-port-cli -- orphans              # 列出孤儿进程
cargo run -p byte-port-cli -- orphans --kill 1234  # 清理指定孤儿（级联子进程）
cargo run -p byte-port-cli -- ls                   # 列全部端口

# --- GUI（建议以管理员运行以获取完整进程/端口信息）---
cargo run -p byte-port-gui --release
# 产物：target/release/byte-port-gui.exe
```

## 技术实现
- **端口枚举**：Windows `iphlpapi` 的 `GetExtendedTcpTable` / `GetExtendedUdpTable`
  （`TCP_TABLE_OWNER_PID_ALL`），自绘 `repr(C)` 结构体直读，零额外进程开销
- **进程信息**：`sysinfo` 跨平台枚举、kill；进程树/孤儿检测基于 `Process::parent()`
- **GUI**：gpui-kit 0.8（Zed GPUI 渲染，crates.io 发布）；扫描走 `cx.spawn` +
  `background_spawn` 线程池 + revision 防过期；DataTable 用 TableDelegate 自定义列与操作按钮；
  弹窗用 `open_dialog`（子树预览）/ `open_alert_dialog`（全部清理）
- **主题**：Catppuccin Mocha（`themes/catppuccin.json` 内嵌，编译进二进制）
- **中文**：Windows 上 GPUI 走 DirectWrite 系统字体回退，零配置


## 后续（见架构文档）
- M2 自动刷新/排序体验打磨
- M3 文件/文件夹占用检测（Windows RestartManager）
- M4 Linux/macOS 适配（`ss`/`lsof`）

> 注：本原型以 Windows 为首要目标，`port_scanner.rs` 已用 `cfg(windows)` 隔离，
> 非 Windows 平台为待实现占位。系统调用模块与 UI 框架解耦，可独立替换。
