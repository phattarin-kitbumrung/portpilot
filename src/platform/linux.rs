use crate::error::PortPilotError;
use crate::models::{PortBinding, ProcessDetails};
use crate::platform::PlatformAdapter;
use crate::platform::support;

pub struct LinuxAdapter;

impl PlatformAdapter for LinuxAdapter {
    fn list_bindings(&self) -> Result<Vec<PortBinding>, PortPilotError> {
        support::list_bindings()
    }

    fn inspect_process(&self, pid: u32) -> Result<ProcessDetails, PortPilotError> {
        support::inspect_process(pid)
    }

    fn kill_process(&self, pid: u32) -> Result<(), PortPilotError> {
        support::kill_process(pid)
    }
}
