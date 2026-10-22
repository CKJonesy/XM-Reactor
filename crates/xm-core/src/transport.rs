//! Byte transport and connector abstractions, so the runner can be tested without Bluetooth.

use std::io;
use std::time::Duration;

pub trait Transport: Send {
    /// Reads whatever is available. A timeout is `Err(TimedOut | WouldBlock)`; `Ok(0)` means closed.
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    BluetoothOff,
    NoDevice,
    NotConnected,
    InUse,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub address: String,
}

pub trait Connector: Send {
    fn connect(
        &mut self,
        preferred: Option<&str>,
    ) -> Result<(Box<dyn Transport>, DeviceInfo), ConnectError>;
}
