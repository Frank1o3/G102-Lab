//! HID++ feature services. Feature indexes are supplied by Root discovery.

pub mod dpi;
pub mod onboard;
pub mod report_rate;
pub mod rgb;

use std::fmt;

use crate::{error::ClientError, protocol::FeatureId};

#[derive(Debug)]
pub enum FeatureError {
    Client(ClientError),
    Unsupported(FeatureId),
    InsufficientReportCapacity {
        feature: FeatureId,
        report_id: u8,
    },
    Malformed(&'static str),
    UnsupportedRgbEffect {
        index: u8,
        id: u16,
    },
    InvalidSensor {
        requested: u8,
        count: u8,
    },
    UnsupportedDpi {
        requested: u16,
    },
    UnsupportedRate {
        requested_hz: u16,
        supported_hz: Vec<u16>,
    },
    VerificationMismatch {
        setting: &'static str,
        requested: u16,
        actual: u16,
    },
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => error.fmt(f),
            Self::Unsupported(id) => write!(f, "device does not advertise feature {id}"),
            Self::InsufficientReportCapacity { feature, report_id } => write!(f, "report 0x{report_id:02x} cannot safely carry the supported-value response for feature {feature}"),
            Self::Malformed(reason) => write!(f, "malformed feature response: {reason}"),
            Self::UnsupportedRgbEffect { index, id } => write!(f, "cluster effect index {index} is device effect 0x{id:04x}; only static color effect 0x0001 is enabled. Run `rgb effects` and select its static-color index"),
            Self::InvalidSensor { requested, count } => write!(
                f,
                "sensor {requested} is outside the device-reported sensor count {count}"
            ),
            Self::UnsupportedDpi { requested } => write!(
                f,
                "DPI {requested} is not in the device-reported supported range or list"
            ),
            Self::UnsupportedRate {
                requested_hz,
                supported_hz,
            } => write!(
                f,
                "{requested_hz} Hz is unsupported; device reports {}",
                supported_hz
                    .iter()
                    .map(|rate| format!("{rate} Hz"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::VerificationMismatch {
                setting,
                requested,
                actual,
            } => write!(
                f,
                "device reported {actual} {setting} after {requested} was requested"
            ),
        }
    }
}

impl std::error::Error for FeatureError {}

impl From<ClientError> for FeatureError {
    fn from(value: ClientError) -> Self {
        Self::Client(value)
    }
}
