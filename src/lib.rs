//! Logitech HID++ discovery, inspection, and explicit feature operations.
//!
//! DPI, report-rate, and RGB setters are explicit operations whose behavior
//! remains hardware-unverified for the target G102. Physical onboard-memory
//! writes remain hard-gated. All communication is serialized through one
//! mutable client per HID endpoint.

pub mod config;
pub mod device;
pub mod error;
pub mod features;
pub mod hid;
pub mod protocol;

pub use device::{ConnectedDevice, DeviceInspection, connect, discover};
pub use error::{ClientError, HidppDeviceError, ProtocolError};
pub use hid::{HidTransport, HidapiTransport, HidppDeviceInfo};
pub use protocol::{FeatureDescriptor, FeatureId, HidppClient, Packet, ReportKind};
