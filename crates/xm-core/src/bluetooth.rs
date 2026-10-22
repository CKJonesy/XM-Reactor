//! Windows RFCOMM transport: finds the paired headset's Sony v2 service and opens a socket to it.

use crate::transport::{ConnectError, Connector, DeviceInfo, Transport};
use std::io;
use std::sync::mpsc;
use std::time::Duration;
use windows::core::{Error, GUID};
use windows::Devices::Bluetooth::BluetoothConnectionStatus;
use windows::Devices::Bluetooth::Rfcomm::{RfcommDeviceService, RfcommServiceId};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const SERVICE_UUID: GUID = GUID::from_u128(0x956C7B26_D49A_4BA8_B03F_B17D393CB6E2);

#[derive(Default)]
pub struct RfcommConnector;

fn other(e: Error) -> ConnectError {
    ConnectError::Other(e.to_string())
}

/// Socket failures worth telling the user about; anything unrecognised stays `Other`.
fn map_connect_error(e: Error) -> ConnectError {
    match e.code().0 as u32 {
        // Access denied, sharing violation, address in use, connection refused.
        0x8007_0005 | 0x8007_0020 | 0x8007_2740 | 0x8007_274D => ConnectError::InUse,
        // Timed out, host down or unreachable, network unreachable, service not found (no link).
        0x8007_274C | 0x8007_2750 | 0x8007_2751 | 0x8007_2742 | 0x8007_277C => {
            ConnectError::NotConnected
        }
        _ => other(e),
    }
}

fn format_address(a: u64) -> String {
    let b = a.to_be_bytes();
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        b[2], b[3], b[4], b[5], b[6], b[7]
    )
}

/// Preferred address first (even if disconnected), then the first connected device, then the first.
fn choose(candidates: &[(String, bool)], preferred: Option<&str>) -> Option<usize> {
    let wanted = preferred.and_then(|p| {
        candidates
            .iter()
            .position(|(a, _)| a.eq_ignore_ascii_case(p))
    });
    wanted
        .or_else(|| candidates.iter().position(|(_, c)| *c))
        .or(if candidates.is_empty() { None } else { Some(0) })
}

/// False only when a Bluetooth radio is present and switched off.
fn radio_on() -> bool {
    let radios = Radio::GetRadiosAsync().and_then(|op| op.join());
    match radios {
        Ok(list) => bluetooth_on(
            &list
                .into_iter()
                .filter(|r| r.Kind() == Ok(RadioKind::Bluetooth))
                .filter_map(|r| r.State().ok())
                .collect::<Vec<_>>(),
        ),
        // The radio API being unavailable is not proof the radio is off.
        Err(_) => true,
    }
}

/// An empty list (no radio reported, as some adapters do) is not proof that Bluetooth is off.
fn bluetooth_on(states: &[RadioState]) -> bool {
    states.is_empty()
        || states
            .iter()
            .any(|s| *s != RadioState::Off && *s != RadioState::Disabled)
}

/// Every paired device exposing the Sony v2 service, with its connection state.
fn enumerate() -> Result<Vec<(RfcommDeviceService, bool, DeviceInfo)>, ConnectError> {
    // Already-initialised (any mode) is fine; WinRT only needs the thread to be in an apartment.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    if !radio_on() {
        return Err(ConnectError::BluetoothOff);
    }
    let id = RfcommServiceId::FromUuid(SERVICE_UUID).map_err(other)?;
    let selector = RfcommDeviceService::GetDeviceSelector(&id).map_err(other)?;
    let found = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .and_then(|op| op.join())
        .map_err(other)?;

    let mut candidates = Vec::new();
    for d in found {
        let Ok(service) =
            RfcommDeviceService::FromIdAsync(&d.Id().map_err(other)?).and_then(|op| op.join())
        else {
            continue;
        };
        let Ok(device) = service.Device() else {
            continue;
        };
        let address = format_address(device.BluetoothAddress().map_err(other)?);
        let connected = device.ConnectionStatus() == Ok(BluetoothConnectionStatus::Connected);
        let name = device.Name().map(|n| n.to_string()).unwrap_or_default();
        candidates.push((service, connected, DeviceInfo { name, address }));
    }
    Ok(candidates)
}

/// Paired devices exposing the service, and whether each is connected right now.
pub fn list_devices() -> Result<Vec<(DeviceInfo, bool)>, ConnectError> {
    Ok(enumerate()?
        .into_iter()
        .map(|(_, up, info)| (info, up))
        .collect())
}

impl Connector for RfcommConnector {
    fn connect(
        &mut self,
        preferred: Option<&str>,
    ) -> Result<(Box<dyn Transport>, DeviceInfo), ConnectError> {
        let mut candidates = enumerate()?;
        let ranked: Vec<(String, bool)> = candidates
            .iter()
            .map(|(_, c, i)| (i.address.clone(), *c))
            .collect();
        let pick = choose(&ranked, preferred).ok_or(ConnectError::NoDevice)?;
        let (service, connected, info) = candidates.swap_remove(pick);
        if !connected {
            return Err(ConnectError::NotConnected);
        }

        let socket = StreamSocket::new().map_err(other)?;
        socket
            .ConnectAsync(
                &service.ConnectionHostName().map_err(other)?,
                &service.ConnectionServiceName().map_err(other)?,
            )
            .and_then(|op| op.join())
            .map_err(map_connect_error)?;
        let transport = RfcommTransport::new(socket).map_err(other)?;
        Ok((Box::new(transport), info))
    }
}

struct RfcommTransport {
    socket: StreamSocket,
    writer: DataWriter,
    rx: mpsc::Receiver<Vec<u8>>,
    /// Bytes received but not yet handed to a caller whose buffer was too small.
    spare: Vec<u8>,
}

impl RfcommTransport {
    fn new(socket: StreamSocket) -> windows::core::Result<Self> {
        let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;
        let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
        let (tx, rx) = mpsc::channel();
        // Ends when the socket closes (LoadAsync fails) or the transport is dropped.
        std::thread::spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            while let Ok(n) = reader.LoadAsync(1024).and_then(|op| op.join()) {
                if n == 0 {
                    break;
                }
                let mut bytes = vec![0u8; n as usize];
                if reader.ReadBytes(&mut bytes).is_err() || tx.send(bytes).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            socket,
            writer,
            rx,
            spare: Vec::new(),
        })
    }
}

impl Transport for RfcommTransport {
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.spare.is_empty() {
            self.spare = match self.rx.recv_timeout(timeout) {
                Ok(b) => b,
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(io::ErrorKind::TimedOut.into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(0),
            };
        }
        let n = self.spare.len().min(buf.len());
        buf[..n].copy_from_slice(&self.spare[..n]);
        self.spare.drain(..n);
        Ok(n)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let io_err = |e: Error| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string());
        self.writer.WriteBytes(bytes).map_err(io_err)?;
        self.writer
            .StoreAsync()
            .and_then(|op| op.join())
            .map_err(io_err)?;
        Ok(())
    }
}

impl Drop for RfcommTransport {
    fn drop(&mut self) {
        // Closing unblocks the reader thread.
        let _ = self.socket.Close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_counts_as_on_unless_a_radio_is_off() {
        let (on, off, disabled) = (RadioState::On, RadioState::Off, RadioState::Disabled);
        assert!(bluetooth_on(&[]));
        assert!(bluetooth_on(&[on]));
        assert!(bluetooth_on(&[off, on]));
        assert!(!bluetooth_on(&[off]));
        assert!(!bluetooth_on(&[off, disabled]));
    }

    #[test]
    fn address_formats_as_colon_hex() {
        assert_eq!(format_address(0x0000_0243_1D55_0D7D), "02:43:1D:55:0D:7D");
    }

    #[test]
    fn choose_ranks_preferred_then_connected_then_first() {
        let c = |a: &str, up| (a.to_string(), up);
        let list = [c("A", false), c("B", true), c("C", true)];
        assert_eq!(choose(&list, Some("a")), Some(0));
        assert_eq!(choose(&list, Some("C")), Some(2));
        assert_eq!(choose(&list, Some("Z")), Some(1));
        assert_eq!(choose(&list, None), Some(1));
        assert_eq!(choose(&[c("A", false), c("B", false)], None), Some(0));
        assert_eq!(choose(&[], None), None);
    }

    #[test]
    fn hresults_map_to_connect_errors() {
        let e = |h: u32| Error::from_hresult(windows::core::HRESULT(h as i32));
        assert_eq!(map_connect_error(e(0x8007_0005)), ConnectError::InUse);
        assert_eq!(
            map_connect_error(e(0x8007_2751)),
            ConnectError::NotConnected
        );
        assert!(matches!(
            map_connect_error(e(0x8000_4005)),
            ConnectError::Other(_)
        ));
    }
}
