mod support;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::error::PortPilotError;
use crate::models::{PortBinding, ProcessDetails};

pub trait PlatformAdapter: Send + Sync {
    fn list_bindings(&self) -> Result<Vec<PortBinding>, PortPilotError>;
    fn inspect_process(&self, pid: u32) -> Result<ProcessDetails, PortPilotError>;
    fn kill_process(&self, pid: u32) -> Result<(), PortPilotError>;
}

#[cfg(target_os = "linux")]
pub fn default_adapter() -> Box<dyn PlatformAdapter> {
    Box::new(linux::LinuxAdapter)
}

#[cfg(target_os = "macos")]
pub fn default_adapter() -> Box<dyn PlatformAdapter> {
    Box::new(macos::MacOsAdapter)
}

#[cfg(target_os = "windows")]
pub fn default_adapter() -> Box<dyn PlatformAdapter> {
    Box::new(windows::WindowsAdapter)
}
