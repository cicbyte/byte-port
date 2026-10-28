# byte-port

> Windows 端口与进程管理工具：搜端口 → 看详情 → 级联结束进程树 → 清理孤儿进程。Rust 原生实现，GUI + CLI 双前端。

[English](README_EN.md) | 简体中文

![Rust](https://img.shields.io/badge/Rust-1.75+-dea584?logo=rust)
![Platform](https://img.shields.io/badge/Platform-Windows-0078D6?logo=windows)
![GUI](https://img.shields.io/badge/GUI-gpui--kit%200.8-DEA584)
![License](https://img.shields.io/badge/License-MIT-yellow)

## 目录

- [功能特性](#功能特性)
- [快速开始](#快速开始)
- [CLI 用法](#cli-用法)
- [GUI](#gui)
- [工作原理](#工作原理)
- [平台说明](#平台说明)
- [许可证](#许可证)

## 功能特性

- **跨字段搜索** — 一个关键字同时匹配端口号、PID、进程名、可执行路径、地址，秒级定位「谁占了我的端口」
- **级联结束进程树** — 结束进程时先预览整棵子进程树（如 bash → node → esbuild），确认后先子后父全部清理，不留新孤儿
- **孤儿进程检测与清理** — 找出父进程已退出但仍在运行的残留进程（Windows 无收养机制，最容易堆积），支持逐个或全部清理
- **GUI + CLI 双前端** — 图形界面适合日常点选，命令行适合脚本与终端流，共用同一核心库
- **原生性能** — 端口枚举直读 Windows `iphlpapi`（`repr(C)` 结构体 FFI），零子进程开销；零 web 技术，单文件二进制

## 快速开始

```bash
git clone https://github.com/cicbyte/byte-port.git
cd byte-port
cargo build --release --workspace
```

产物：

| 文件 | 说明 |
|---|---|
| `target/release/byte-port-cli.exe` | 命令行版（约 1 MB） |
| `target/release/byte-port-gui.exe` | 图形界面版（约 24 MB） |

```bash
# CLI：三步走完核心工作流
byte-port-cli search 8080        # 搜：谁占用了 8080
byte-port-cli show 1588          # 看：这个进程的所有端口和路径
byte-port-cli kill 1588          # 杀：预览子树 → 确认级联结束

# GUI
byte-port-gui.exe                # 建议以管理员运行，可获取完整进程信息
```

## CLI 用法

主工作流：**search 定位 → show 看详情 → kill 关闭**；另支持 orphans 孤儿清理。

### 全部命令

| 命令 | 说明 |
|---|---|
| `search <关键字>` | 跨字段搜索（端口号/PID/进程名/路径/地址） |
| `show <pid>` | 查看某进程占用的全部端口 + 可执行路径 |
| `kill <pid>` | 级联结束进程及其子进程（执行前预览） |
| `orphans` | 检测/清理孤儿进程（父进程已退出者） |
| `ls [关键字]` | 列出端口（兜底命令，有关键字则搜索） |

### `search` — 搜索定位

```bash
$ byte-port-cli search svchost --listen
协议  本地地址:端口     状态    PID   进程名
---  ---------------  ------  ----  -----------
TCP  0.0.0.0:135      LISTEN  1588  svchost.exe
...

# 选项：--listen 仅 TCP 监听 / --proto tcp|udp / --json 机器可读
```

### `show` — 进程详情

```bash
$ byte-port-cli show 1588
PID      : 1588
进程名   : svchost.exe
可执行   : (未知)          # 系统进程需管理员权限才可见路径
占用端口 : 1
```

### `kill` — 级联结束

```bash
$ byte-port-cli kill 61076
即将结束 PID 61076（含子进程，共 6 个进程）:
  ├ PID 61076  bash.exe
    ├ PID 61260  bash.exe
      ├ PID 38652  node.exe
        ...
⚠ 将级联结束以上子进程
确认结束? [y/N] y
完成：成功 6 / 失败 0
```

`-f / --force` 跳过确认（仍级联）。

### `orphans` — 孤儿检测与清理

```bash
byte-port-cli orphans                    # 列出全部孤儿（PID/进程名/路径/子进程数）
byte-port-cli orphans --json             # 机器可读
byte-port-cli orphans --kill 52868       # 清理指定孤儿（级联其子树）
byte-port-cli orphans --kill-all         # 清理全部（二次确认）
```

## GUI

基于 [gpui-kit](https://github.com/longbridge/gpui-kit)（Zed GPUI 渲染）+ Catppuccin Mocha 深色主题，百度式双态界面：

- **首页态** — 居中大搜索框，启动不预扫、零信息干扰；回车或点击搜索才触发扫描
- **结果态** — 端口 / 孤儿双标签页（各自独立缓存，切换不重扫）；DataTable 表格，行内「结束 / 清理」按钮
- **安全确认** — 所有结束操作弹窗预览整棵子进程树，二次确认后级联执行
- **中文零配置** — Windows 上经 DirectWrite 系统字体回退，无需打包字体文件

## 工作原理

```
crates/core（byte-port-core）          apps/
├─ port_scanner  iphlpapi FFI 直读    ├─ gui/   gpui-kit 前端
├─ proc_tree     进程树/孤儿检测      └─ cli/   clap 前端
├─ filter        跨字段筛选
└─ query         按端口/PID 查询
```

- **端口枚举**：`GetExtendedTcpTable` / `GetExtendedUdpTable`（`TCP_TABLE_OWNER_PID_ALL`），自绘 `repr(C)` 结构体直读，不依赖 windows crate
- **进程关联与结束**：`sysinfo` 枚举进程名/路径，`Process::parent()` 构建进程树
- **GUI 线程模型**：扫描走 `cx.spawn` + `background_spawn` 线程池，revision 计数丢弃过期结果，UI 永不卡顿
- **系统调用层与 UI 完全解耦**：替换平台后端或 UI 框架互不影响

## 平台说明

**目前仅支持 Windows**（端口枚举走 `iphlpapi`，非 Windows 平台为编译通过的空占位）。

关于孤儿进程：Linux/macOS 由 init/launchd（PID 1）自动收养孤儿，被视为正常机制；**Windows 无收养机制**，父进程退出后子进程真正孤立，最需要手动清理——这也是本工具聚焦 Windows 的原因之一。检测逻辑基于 `sysinfo::Process::parent()`，跨平台通用，未来适配 `ss` / `lsof` 后即可扩展。

## 许可证

[MIT](LICENSE) © 2026 cicbyte
