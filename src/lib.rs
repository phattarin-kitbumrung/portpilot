pub mod error;
pub mod models;
mod platform;

use std::collections::HashSet;

use error::PortPilotError;
use models::{PortBinding, ProcessDetails, TerminationResult, TerminationTarget};
use platform::{PlatformAdapter, default_adapter};

pub struct PortPilot {
    adapter: Box<dyn PlatformAdapter>,
}

impl Default for PortPilot {
    fn default() -> Self {
        Self {
            adapter: default_adapter(),
        }
    }
}

impl PortPilot {
    #[cfg(test)]
    pub(crate) fn with_adapter(adapter: Box<dyn PlatformAdapter>) -> Self {
        Self { adapter }
    }

    pub fn list_ports(&self) -> Result<Vec<PortBinding>, PortPilotError> {
        let mut bindings = self.adapter.list_bindings()?;
        bindings.sort_by_key(|binding| (binding.port, binding.pid));
        bindings.dedup_by(|a, b| a.port == b.port && a.pid == b.pid && a.protocol == b.protocol);
        Ok(bindings)
    }

    pub fn find_by_port(&self, port: u16) -> Result<PortBinding, PortPilotError> {
        self.list_ports()?
            .into_iter()
            .find(|binding| binding.port == port)
            .ok_or(PortPilotError::PortNotFound(port))
    }

    pub fn find_by_process(&self, query: &str) -> Result<Vec<PortBinding>, PortPilotError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(PortPilotError::InvalidInput(
                "process query cannot be empty".to_string(),
            ));
        }

        let bindings = self.list_ports()?;
        let filtered: Vec<PortBinding> = match query.parse::<u32>() {
            Ok(pid) => bindings
                .into_iter()
                .filter(|binding| binding.pid == pid)
                .collect(),
            Err(_) => {
                let needle = query.to_ascii_lowercase();
                bindings
                    .into_iter()
                    .filter(|binding| binding.process.to_ascii_lowercase().contains(&needle))
                    .collect()
            }
        };

        if filtered.is_empty() {
            return Err(PortPilotError::ProcessNotFound(query.to_string()));
        }

        Ok(filtered)
    }

    pub fn inspect_pid(&self, pid: u32) -> Result<ProcessDetails, PortPilotError> {
        let mut details = self.adapter.inspect_process(pid)?;
        if details.ports.is_empty() {
            let ports: HashSet<u16> = self
                .list_ports()?
                .into_iter()
                .filter(|binding| binding.pid == pid)
                .map(|binding| binding.port)
                .collect();
            details.ports = ports.into_iter().collect();
            details.ports.sort_unstable();
        }

        Ok(details)
    }

    pub fn terminate(
        &self,
        target: TerminationTarget,
    ) -> Result<TerminationResult, PortPilotError> {
        let pid = match target {
            TerminationTarget::Pid(pid) => pid,
            TerminationTarget::Port(port) => self.find_by_port(port)?.pid,
        };

        let before_ports: HashSet<u16> = self
            .list_ports()?
            .into_iter()
            .filter(|binding| binding.pid == pid)
            .map(|binding| binding.port)
            .collect();

        self.adapter.kill_process(pid)?;

        Ok(TerminationResult {
            pid,
            freed_ports: before_ports.into_iter().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PortBinding, ProcessDetails, Protocol};

    struct MockAdapter;

    impl PlatformAdapter for MockAdapter {
        fn list_bindings(&self) -> Result<Vec<PortBinding>, PortPilotError> {
            Ok(vec![
                PortBinding {
                    port: 3000,
                    protocol: Protocol::Tcp,
                    pid: 10,
                    process: "node".to_string(),
                    status: Some("LISTEN".to_string()),
                    cpu_percent: 12.4,
                    memory_bytes: 190_000_000,
                },
                PortBinding {
                    port: 5432,
                    protocol: Protocol::Tcp,
                    pid: 11,
                    process: "postgres".to_string(),
                    status: Some("LISTEN".to_string()),
                    cpu_percent: 2.1,
                    memory_bytes: 512_000_000,
                },
            ])
        }

        fn inspect_process(&self, pid: u32) -> Result<ProcessDetails, PortPilotError> {
            Ok(ProcessDetails {
                pid,
                process: "node".to_string(),
                command: "node server.js".to_string(),
                user: Some("tester".to_string()),
                ports: vec![3000],
                cpu_percent: 12.4,
                memory_bytes: 190_000_000,
            })
        }

        fn kill_process(&self, _pid: u32) -> Result<(), PortPilotError> {
            Ok(())
        }
    }

    #[test]
    fn finds_process_by_name() {
        let pilot = PortPilot::with_adapter(Box::new(MockAdapter));
        let result = pilot.find_by_process("node").expect("expected process");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pid, 10);
    }

    #[test]
    fn terminates_by_port() {
        let pilot = PortPilot::with_adapter(Box::new(MockAdapter));
        let result = pilot
            .terminate(TerminationTarget::Port(3000))
            .expect("termination should pass");
        assert_eq!(result.pid, 10);
        assert!(result.freed_ports.contains(&3000));
    }
}
