//! Read-only Logitech HID++ discovery and inspection.
//!
//! This crate deliberately exposes no device-setting or onboard-memory write
//! APIs. All communication is serialized through one mutable client per HID
//! endpoint.

pub mod device;
pub mod error;
pub mod features;
pub mod hid;
pub mod protocol;

pub use device::{connect, discover, ConnectedDevice, DeviceInspection};
pub use error::{ClientError, HidppDeviceError, ProtocolError};
pub use hid::{HidTransport, HidapiTransport, HidppDeviceInfo};
pub use protocol::{FeatureDescriptor, FeatureId, HidppClient, Packet, ReportKind};
