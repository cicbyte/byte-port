use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone)]
pub struct PortEntry {
    pub protocol: Protocol,
    pub local_addr: String, // "127.0.0.1:8080"
    pub local_ip: String,
    pub local_port: u16,
    pub state: String, // LISTEN / ESTABLISHED / "-" (udp)
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub exe_path: Option<PathBuf>,
}
