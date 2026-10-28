# byte-port

> Windows port & process manager: search ports → inspect details → cascade-kill process trees → clean up orphans. Native Rust, GUI + CLI frontends.

English | [简体中文](README.md)

![Rust](https://img.shields.io/badge/Rust-1.75+-dea584?logo=rust)
![Platform](https://img.shields.io/badge/Platform-Windows-0078D6?logo=windows)
![GUI](https://img.shields.io/badge/GUI-gpui--kit%200.8-DEA584)
![License](https://img.shields.io/badge/License-MIT-yellow)

## Table of Contents

- [Features](#features)
- [Quick Start](#quick-start)
- [CLI Usage](#cli-usage)
- [GUI](#gui)
- [How It Works](#how-it-works)
- [Platform Notes](#platform-notes)
- [License](#license)

## Features

- **Cross-field search** — one keyword matches port numbers, PIDs, process names, executable paths, and addresses; locate "who's holding my port" in seconds
- **Cascade process-tree kill** — preview the entire subtree (e.g. bash → node → esbuild) before terminating; children first, then the parent, leaving no new orphans behind
- **Orphan detection & cleanup** — find leftover processes whose parent has exited (Windows has no adoption mechanism, so they pile up); clean individually or all at once
- **GUI + CLI frontends** — point-and-click for daily use, commands for scripts and terminals, sharing one core library
- **Native performance** — port enumeration reads Windows `iphlpapi` directly (`repr(C)` struct FFI), zero subprocess overhead; no web tech, single-file binaries

## Quick Start

```bash
git clone https://github.com/cicbyte/byte-port.git
cd byte-port
cargo build --release --workspace
```

Artifacts:

| File | Description |
|---|---|
| `target/release/byte-port-cli.exe` | CLI (~1 MB) |
| `target/release/byte-port-gui.exe` | GUI (~24 MB) |

```bash
# CLI: the core workflow in three steps
byte-port-cli search 8080        # Search: who is using port 8080
byte-port-cli show 1588          # Inspect: all ports and paths of this process
byte-port-cli kill 1588          # Kill: preview subtree → confirm cascade

# GUI
byte-port-gui.exe                # Run as administrator for full process info
```

## CLI Usage

Main workflow: **search → show → kill**, plus orphan cleanup.

### Commands

| Command | Description |
|---|---|
| `search <keyword>` | Cross-field search (port/PID/process name/path/address) |
| `show <pid>` | All ports + executable path of a process |
| `kill <pid>` | Cascade-terminate a process and its children (with preview) |
| `orphans` | Detect / clean orphan processes (parent already exited) |
| `ls [keyword]` | List ports (fallback; searches when a keyword is given) |

### `search`

```bash
$ byte-port-cli search svchost --listen
PROTO  LOCAL ADDRESS     STATE    PID   NAME
---    ---------------  -------  ----  -----------
TCP    0.0.0.0:135      LISTEN   1588  svchost.exe
...

# Options: --listen TCP listeners only / --proto tcp|udp / --json
```

### `show`

```bash
$ byte-port-cli show 1588
PID      : 1588
Name     : svchost.exe
Exe      : (unknown)       # system processes need admin rights for paths
Ports    : 1
```

### `kill` — cascade termination

```bash
$ byte-port-cli kill 61076
Terminating PID 61076 (6 processes including children):
  ├ PID 61076  bash.exe
    ├ PID 61260  bash.exe
      ├ PID 38652  node.exe
        ...
⚠ Children above will be terminated as well
Confirm? [y/N] y
Done: 6 ok / 0 failed
```

`-f / --force` skips the confirmation (still cascades).

### `orphans`

```bash
byte-port-cli orphans                    # list all orphans (PID/name/path/children)
byte-port-cli orphans --json             # machine-readable
byte-port-cli orphans --kill 52868       # clean one orphan (cascades its subtree)
byte-port-cli orphans --kill-all         # clean all (double confirmation)
```

## GUI

Built on [gpui-kit](https://github.com/longbridge/gpui-kit) (Zed's GPUI renderer) with the Catppuccin Mocha dark theme, in a Baidu-style two-state layout:

- **Home state** — a large centered search box; no pre-scan on launch, zero noise; scanning starts only on Enter or click
- **Result state** — Ports / Orphans tabs (independent caches, no rescan on switch); DataTable with inline Kill / Clean buttons
- **Safe confirmation** — every termination previews the full process subtree in a dialog before cascading
- **CJK out of the box** — DirectWrite system font fallback on Windows, no bundled font files

## How It Works

```
crates/core (byte-port-core)           apps/
├─ port_scanner  iphlpapi FFI          ├─ gui/   gpui-kit frontend
├─ proc_tree     process tree/orphans  └─ cli/   clap frontend
├─ filter        cross-field filtering
└─ query         by-port / by-PID lookup
```

- **Port enumeration**: `GetExtendedTcpTable` / `GetExtendedUdpTable` (`TCP_TABLE_OWNER_PID_ALL`), read via hand-drawn `repr(C)` structs — no windows crate dependency
- **Process association & termination**: `sysinfo` for names/paths, `Process::parent()` to build process trees
- **GUI threading**: scans run on `cx.spawn` + `background_spawn` thread pool; a revision counter discards stale results; the UI never blocks
- **Syscall layer fully decoupled from UI**: platform backend and UI framework can be swapped independently

## Platform Notes

**Windows only for now** (port enumeration uses `iphlpapi`; other platforms compile as empty stubs).

About orphans: Linux/macOS have init/launchd (PID 1) automatically adopt orphans, which is considered normal; **Windows has no adoption mechanism** — when the parent exits, children are truly stranded and need manual cleanup. That's one reason this tool focuses on Windows. The detection logic (`sysinfo::Process::parent()`) is cross-platform; adapting `ss` / `lsof` later will unlock other platforms.

## License

[MIT](LICENSE) © 2026 cicbyte
