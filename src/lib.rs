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
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockAdapter {
        bindings: Vec<PortBinding>,
        inspect_ports: Vec<u16>,
        fail_list: bool,
        fail_kill: bool,
        killed: Mutex<Vec<u32>>,
    }

    fn binding(port: u16, pid: u32, process: &str, protocol: Protocol) -> PortBinding {
        PortBinding {
            port,
            protocol,
            pid,
            process: process.to_string(),
            status: Some("LISTEN".to_string()),
            cpu_percent: 1.0,
            memory_bytes: 1024,
        }
    }

    fn default_bindings() -> Vec<PortBinding> {
        vec![
            binding(5432, 11, "postgres", Protocol::Tcp),
            binding(3000, 10, "Node", Protocol::Tcp),
            binding(3001, 10, "Node", Protocol::Tcp),
        ]
    }

    impl PlatformAdapter for MockAdapter {
        fn list_bindings(&self) -> Result<Vec<PortBinding>, PortPilotError> {
            if self.fail_list {
                return Err(PortPilotError::Platform("boom".to_string()));
            }
            Ok(self.bindings.clone())
        }

        fn inspect_process(&self, pid: u32) -> Result<ProcessDetails, PortPilotError> {
            if pid == 999 {
                return Err(PortPilotError::PidNotFound(pid));
            }
            Ok(ProcessDetails {
                pid,
                process: "node".to_string(),
                command: "node server.js".to_string(),
                user: Some("tester".to_string()),
                ports: self.inspect_ports.clone(),
                cpu_percent: 0.0,
                memory_bytes: 0,
            })
        }

        fn kill_process(&self, pid: u32) -> Result<(), PortPilotError> {
            if self.fail_kill {
                return Err(PortPilotError::OperationFailed("denied".to_string()));
            }
            self.killed.lock().unwrap().push(pid);
            Ok(())
        }
    }

    fn pilot(adapter: MockAdapter) -> PortPilot {
        PortPilot::with_adapter(Box::new(adapter))
    }

    fn standard() -> PortPilot {
        pilot(MockAdapter {
            bindings: default_bindings(),
            ..Default::default()
        })
    }

    #[test]
    fn list_ports_sorts_by_port_then_pid() {
        let ports: Vec<u16> = standard()
            .list_ports()
            .unwrap()
            .iter()
            .map(|b| b.port)
            .collect();
        assert_eq!(ports, vec![3000, 3001, 5432]);
    }

    #[test]
    fn list_ports_removes_duplicate_bindings() {
        let p = pilot(MockAdapter {
            bindings: vec![
                binding(80, 1, "nginx", Protocol::Tcp),
                binding(80, 1, "nginx", Protocol::Tcp),
                binding(80, 1, "nginx", Protocol::Udp),
            ],
            ..Default::default()
        });
        assert_eq!(p.list_ports().unwrap().len(), 2);
    }

    #[test]
    fn list_ports_propagates_adapter_error() {
        let p = pilot(MockAdapter {
            fail_list: true,
            ..Default::default()
        });
        assert!(matches!(p.list_ports(), Err(PortPilotError::Platform(_))));
    }

    #[test]
    fn find_by_port_returns_match() {
        assert_eq!(standard().find_by_port(5432).unwrap().pid, 11);
    }

    #[test]
    fn find_by_port_missing_returns_not_found() {
        assert!(matches!(
            standard().find_by_port(1),
            Err(PortPilotError::PortNotFound(1))
        ));
    }

    #[test]
    fn find_by_process_is_case_insensitive_substring() {
        let found = standard().find_by_process("  NODE ").unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|b| b.pid == 10));
    }

    #[test]
    fn find_by_process_numeric_query_matches_pid() {
        let found = standard().find_by_process("11").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].process, "postgres");
    }

    #[test]
    fn find_by_process_rejects_empty_query() {
        for query in ["", "   "] {
            assert!(matches!(
                standard().find_by_process(query),
                Err(PortPilotError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn find_by_process_unknown_returns_not_found() {
        assert!(matches!(
            standard().find_by_process("ghost"),
            Err(PortPilotError::ProcessNotFound(name)) if name == "ghost"
        ));
        assert!(matches!(
            standard().find_by_process("424242"),
            Err(PortPilotError::ProcessNotFound(_))
        ));
    }

    #[test]
    fn inspect_pid_keeps_adapter_ports_when_present() {
        let p = pilot(MockAdapter {
            bindings: default_bindings(),
            inspect_ports: vec![9999],
            ..Default::default()
        });
        assert_eq!(p.inspect_pid(10).unwrap().ports, vec![9999]);
    }

    #[test]
    fn inspect_pid_fills_missing_ports_sorted() {
        assert_eq!(standard().inspect_pid(10).unwrap().ports, vec![3000, 3001]);
    }

    #[test]
    fn inspect_pid_propagates_not_found() {
        assert!(matches!(
            standard().inspect_pid(999),
            Err(PortPilotError::PidNotFound(999))
        ));
    }

    #[test]
    fn terminate_by_port_kills_owner_and_reports_all_freed_ports() {
        let adapter = MockAdapter {
            bindings: default_bindings(),
            ..Default::default()
        };
        let p = PortPilot::with_adapter(Box::new(adapter));
        let mut result = p.terminate(TerminationTarget::Port(3000)).unwrap();
        result.freed_ports.sort_unstable();
        assert_eq!(result.pid, 10);
        assert_eq!(result.freed_ports, vec![3000, 3001]);
    }

    #[test]
    fn terminate_by_pid_invokes_adapter_once() {
        let adapter = std::sync::Arc::new(MockAdapter {
            bindings: default_bindings(),
            ..Default::default()
        });

        struct Shared(std::sync::Arc<MockAdapter>);
        impl PlatformAdapter for Shared {
            fn list_bindings(&self) -> Result<Vec<PortBinding>, PortPilotError> {
                self.0.list_bindings()
            }
            fn inspect_process(&self, pid: u32) -> Result<ProcessDetails, PortPilotError> {
                self.0.inspect_process(pid)
            }
            fn kill_process(&self, pid: u32) -> Result<(), PortPilotError> {
                self.0.kill_process(pid)
            }
        }

        let p = PortPilot::with_adapter(Box::new(Shared(adapter.clone())));
        let result = p.terminate(TerminationTarget::Pid(11)).unwrap();
        assert_eq!(result.pid, 11);
        assert_eq!(result.freed_ports, vec![5432]);
        assert_eq!(*adapter.killed.lock().unwrap(), vec![11]);
    }

    #[test]
    fn terminate_unknown_port_does_not_kill() {
        assert!(matches!(
            standard().terminate(TerminationTarget::Port(1)),
            Err(PortPilotError::PortNotFound(1))
        ));
    }

    #[test]
    fn terminate_propagates_kill_failure() {
        let p = pilot(MockAdapter {
            bindings: default_bindings(),
            fail_kill: true,
            ..Default::default()
        });
        assert!(matches!(
            p.terminate(TerminationTarget::Pid(10)),
            Err(PortPilotError::OperationFailed(_))
        ));
    }

    #[test]
    fn terminate_unlisted_pid_reports_no_freed_ports() {
        let result = standard().terminate(TerminationTarget::Pid(12345)).unwrap();
        assert!(result.freed_ports.is_empty());
    }

    #[test]
    fn error_messages_are_human_readable() {
        assert_eq!(
            PortPilotError::PortNotFound(80).to_string(),
            "port 80 not found"
        );
        assert_eq!(
            PortPilotError::ProcessNotFound("x".into()).to_string(),
            "process 'x' not found"
        );
        assert_eq!(
            PortPilotError::PidNotFound(7).to_string(),
            "pid 7 not found"
        );
    }
}
