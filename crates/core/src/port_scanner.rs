use std::collections::HashMap;
use sysinfo::{Pid, System};

use crate::model::{PortEntry, Protocol};

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::net::Ipv4Addr;

    // 直接用 Rust 原生 FFI 链接系统 iphlpapi.lib，零额外 crate 依赖。
    type BOOL = i32;
    const FALSE: BOOL = 0;
    const NO_ERROR: u32 = 0;
    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_ALL: u32 = 5;
    const UDP_TABLE_OWNER_PID: u32 = 1;

    // 与 Windows C 布局完全一致的 repr(C) 结构体，直读，不依赖任何 crate 字段定义。
    // 派生 Copy/Clone：便于从裸指针解引用拷贝出元素。
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MibTcpRowOwnerPid {
        pub dw_state: u32,
        pub dw_local_addr: u32,
        pub dw_local_port: u32,
        pub dw_remote_addr: u32,
        pub dw_remote_port: u32,
        pub dw_owning_pid: u32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MibTcpTableOwnerPid {
        pub dw_num_entries: u32,
        pub table: [MibTcpRowOwnerPid; 1],
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MibUdpRowOwnerPid {
        pub dw_local_addr: u32,
        pub dw_local_port: u32,
        pub dw_owning_pid: u32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MibUdpTableOwnerPid {
        pub dw_num_entries: u32,
        pub table: [MibUdpRowOwnerPid; 1],
    }

    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            pTcpTable: *mut c_void,
            pdwSize: *mut u32,
            bOrder: BOOL,
            ulAf: u32,
            TableClass: u32,
            Reserved: u32,
        ) -> u32;
        fn GetExtendedUdpTable(
            pUdpTable: *mut c_void,
            pdwSize: *mut u32,
            bOrder: BOOL,
            ulAf: u32,
            TableClass: u32,
            Reserved: u32,
        ) -> u32;
    }

    pub fn tcp_table() -> Vec<MibTcpRowOwnerPid> {
        let mut size: u32 = 0;
        unsafe {
            let _ = GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                FALSE,
                AF_INET,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
        }
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let res = unsafe {
            GetExtendedTcpTable(
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
                FALSE,
                AF_INET,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if res != NO_ERROR {
            return Vec::new();
        }
        let table = buf.as_ptr() as *const MibTcpTableOwnerPid;
        let n = unsafe { (*table).dw_num_entries };
        let base = unsafe { &(*table).table as *const MibTcpRowOwnerPid };
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            unsafe {
                out.push(*base.add(i as usize));
            }
        }
        out
    }

    pub fn udp_table() -> Vec<MibUdpRowOwnerPid> {
        let mut size: u32 = 0;
        unsafe {
            let _ = GetExtendedUdpTable(
                std::ptr::null_mut(),
                &mut size,
                FALSE,
                AF_INET,
                UDP_TABLE_OWNER_PID,
                0,
            );
        }
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let res = unsafe {
            GetExtendedUdpTable(
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
                FALSE,
                AF_INET,
                UDP_TABLE_OWNER_PID,
                0,
            )
        };
        if res != NO_ERROR {
            return Vec::new();
        }
        let table = buf.as_ptr() as *const MibUdpTableOwnerPid;
        let n = unsafe { (*table).dw_num_entries };
        let base = unsafe { &(*table).table as *const MibUdpRowOwnerPid };
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            unsafe {
                out.push(*base.add(i as usize));
            }
        }
        out
    }

    // dwLocalPort 以网络字节序(大端)存于低 16 位
    pub fn port_from_raw(v: u32) -> u16 {
        u16::from_be((v & 0xFFFF) as u16)
    }
    pub fn ip_from_raw(v: u32) -> String {
        Ipv4Addr::from(v).to_string()
    }
}

fn tcp_state_name(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RCVD",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT1",
        7 => "FIN_WAIT2",
        8 => "TIME_WAIT",
        9 => "CLOSE_WAIT",
        10 => "LAST_ACK",
        11 => "CLOSING",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
}

/// 扫描全部 TCP/UDP 端口并关联进程信息（Windows: iphlpapi；其余平台走 ss/lsof 兜底）
pub fn scan_all() -> Vec<PortEntry> {
    let mut entries = Vec::new();

    #[cfg(windows)]
    {
        use win::*;
        let sys = System::new_all();
        let mut names: HashMap<u32, (String, Option<std::path::PathBuf>)> = HashMap::new();
        for (pid, p) in sys.processes() {
            names.insert(
                pid.as_u32(),
                (p.name().to_string_lossy().to_string(), p.exe().map(|x| x.to_path_buf())),
            );
        }

        for row in tcp_table() {
            let pid = if row.dw_owning_pid == 0 {
                None
            } else {
                Some(row.dw_owning_pid)
            };
            let (process_name, exe_path) = match pid.and_then(|p| names.get(&p).cloned()) {
                Some((n, e)) => (Some(n), e),
                None => (None, None),
            };
            let ip = ip_from_raw(row.dw_local_addr);
            let port = port_from_raw(row.dw_local_port);
            entries.push(PortEntry {
                protocol: Protocol::Tcp,
                local_addr: format!("{}:{}", ip, port),
                local_ip: ip,
                local_port: port,
                state: tcp_state_name(row.dw_state).to_string(),
                pid,
                process_name,
                exe_path,
            });
        }

        for row in udp_table() {
            let pid = if row.dw_owning_pid == 0 {
                None
            } else {
                Some(row.dw_owning_pid)
            };
            let (process_name, exe_path) = match pid.and_then(|p| names.get(&p).cloned()) {
                Some((n, e)) => (Some(n), e),
                None => (None, None),
            };
            let ip = ip_from_raw(row.dw_local_addr);
            let port = port_from_raw(row.dw_local_port);
            entries.push(PortEntry {
                protocol: Protocol::Udp,
                local_addr: format!("{}:{}", ip, port),
                local_ip: ip,
                local_port: port,
                state: "-".to_string(),
                pid,
                process_name,
                exe_path,
            });
        }
    }

    #[cfg(not(windows))]
    {
        // TODO: 非 Windows 用 `ss -tulpn` / `lsof -i` 解析（参考调研中的 PortKiller）
        let _ = &mut entries; // 抑制未使用警告
    }

    entries.sort_by(|a, b| a.local_port.cmp(&b.local_port));
    entries
}

/// 结束指定 PID 的进程。需要足够权限（建议以管理员运行）
pub fn kill_pid(pid: u32) -> bool {
    let sys = System::new_all();
    if let Some(p) = sys.process(Pid::from_u32(pid)) {
        return p.kill();
    }
    false
}
