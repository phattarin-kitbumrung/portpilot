use std::collections::HashSet;

use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState, get_sockets_info};
use sysinfo::{Pid, Signal, System, Users};

use crate::error::PortPilotError;
use crate::models::{PortBinding, ProcessDetails, Protocol};

pub(crate) fn list_bindings() -> Result<Vec<PortBinding>, PortPilotError> {
    let mut system = System::new_all();
    system.refresh_all();

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP | ProtocolFlags::UDP;
    let sockets = get_sockets_info(af_flags, proto_flags)
        .map_err(|error| PortPilotError::Platform(error.to_string()))?;

    let mut bindings = Vec::new();
    for socket in sockets {
        let (protocol, port, status) = match socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp) => (
                Protocol::Tcp,
                tcp.local_port,
                Some(tcp_state_to_string(tcp.state)),
            ),
            ProtocolSocketInfo::Udp(udp) => (Protocol::Udp, udp.local_port, None),
        };

        for pid in socket.associated_pids {
            let process_name = system
                .process(Pid::from_u32(pid))
                .map(|process| process.name().to_string())
                .unwrap_or_else(|| "<unknown>".to_string());
            let (cpu_percent, memory_bytes) = system
                .process(Pid::from_u32(pid))
                .map(|process| (process.cpu_usage(), process.memory()))
                .unwrap_or((0.0, 0));

            bindings.push(PortBinding {
                port,
                protocol,
                pid,
                process: process_name,
                status: status.clone(),
                cpu_percent,
                memory_bytes,
            });
        }
    }

    Ok(bindings)
}

pub(crate) fn inspect_process(pid: u32) -> Result<ProcessDetails, PortPilotError> {
    let mut system = System::new_all();
    system.refresh_all();
    let users = Users::new_with_refreshed_list();

    let process = system
        .process(Pid::from_u32(pid))
        .ok_or(PortPilotError::PidNotFound(pid))?;

    let user = process
        .user_id()
        .and_then(|uid| users.get_user_by_id(uid))
        .map(|user| user.name().to_string());

    let command = if process.cmd().is_empty() {
        process.name().to_string()
    } else {
        process.cmd().join(" ")
    };

    let bindings = list_bindings()?;
    let ports: HashSet<u16> = bindings
        .into_iter()
        .filter(|binding| binding.pid == pid)
        .map(|binding| binding.port)
        .collect();

    let mut ports = ports.into_iter().collect::<Vec<_>>();
    ports.sort_unstable();

    Ok(ProcessDetails {
        pid,
        process: process.name().to_string(),
        command,
        user,
        ports,
        cpu_percent: process.cpu_usage(),
        memory_bytes: process.memory(),
    })
}

pub(crate) fn kill_process(pid: u32) -> Result<(), PortPilotError> {
    let mut system = System::new_all();
    system.refresh_all();

    let process = system
        .process(Pid::from_u32(pid))
        .ok_or(PortPilotError::PidNotFound(pid))?;

    let terminated = process
        .kill_with(Signal::Term)
        .or_else(|| Some(process.kill()))
        .unwrap_or(false);

    if terminated {
        Ok(())
    } else {
        Err(PortPilotError::OperationFailed(format!(
            "failed to terminate pid {pid}"
        )))
    }
}

fn tcp_state_to_string(state: TcpState) -> String {
    match state {
        TcpState::Listen => "LISTEN".to_string(),
        TcpState::Established => "ESTABLISHED".to_string(),
        TcpState::SynSent => "SYN_SENT".to_string(),
        TcpState::SynReceived => "SYN_RECV".to_string(),
        TcpState::FinWait1 => "FIN_WAIT1".to_string(),
        TcpState::FinWait2 => "FIN_WAIT2".to_string(),
        TcpState::CloseWait => "CLOSE_WAIT".to_string(),
        TcpState::Closed => "CLOSED".to_string(),
        TcpState::LastAck => "LAST_ACK".to_string(),
        TcpState::Closing => "CLOSING".to_string(),
        TcpState::TimeWait => "TIME_WAIT".to_string(),
        TcpState::DeleteTcb => "DELETE_TCB".to_string(),
        TcpState::Unknown => "UNKNOWN".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn tcp_state_names_are_stable() {
        assert_eq!(tcp_state_to_string(TcpState::Listen), "LISTEN");
        assert_eq!(tcp_state_to_string(TcpState::Established), "ESTABLISHED");
        assert_eq!(tcp_state_to_string(TcpState::TimeWait), "TIME_WAIT");
        assert_eq!(tcp_state_to_string(TcpState::Unknown), "UNKNOWN");
    }

    #[test]
    fn list_bindings_sees_own_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let bindings = list_bindings().unwrap();
        let mine = bindings
            .iter()
            .find(|b| b.port == port && b.pid == std::process::id())
            .expect("own listener should be listed");
        assert_eq!(mine.protocol, Protocol::Tcp);
        assert_eq!(mine.status.as_deref(), Some("LISTEN"));
    }

    #[test]
    fn inspect_process_reports_current_process() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let details = inspect_process(std::process::id()).unwrap();
        assert_eq!(details.pid, std::process::id());
        assert!(!details.command.is_empty());
        assert!(details.ports.contains(&port));
    }

    #[test]
    fn inspect_and_kill_missing_pid_return_not_found() {
        assert!(matches!(
            inspect_process(u32::MAX - 1),
            Err(PortPilotError::PidNotFound(_))
        ));
        assert!(matches!(
            kill_process(u32::MAX - 1),
            Err(PortPilotError::PidNotFound(_))
        ));
    }
}
