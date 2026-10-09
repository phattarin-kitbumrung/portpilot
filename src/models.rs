#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PortBinding {
    pub port: u16,
    pub protocol: Protocol,
    pub pid: u32,
    pub process: String,
    pub status: Option<String>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessDetails {
    pub pid: u32,
    pub process: String,
    pub command: String,
    pub user: Option<String>,
    pub ports: Vec<u16>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationTarget {
    Port(u16),
    Pid(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationResult {
    pub pid: u32,
    pub freed_ports: Vec<u16>,
}
