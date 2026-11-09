use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidReportKind(u8),
    InvalidReportLength {
        kind: u8,
        actual: usize,
        expected: usize,
    },
    InvalidFunction(u8),
    InvalidSoftwareId(u8),
    ParameterTooLong {
        kind: u8,
        actual: usize,
        maximum: usize,
    },
    MalformedPacket(&'static str),
    UnexpectedDeviceIndex {
        expected: u8,
        actual: u8,
    },
    Device(HidppDeviceError),
    Timeout,
    FeatureSetTooLarge(u8),
    FeatureSetIndexMismatch {
        feature_id: u16,
        table_index: u8,
        root_index: u8,
    },
    MissingRequiredParameter {
        feature: u16,
        function: u8,
        required: usize,
        actual: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidppDeviceError {
    pub code: u8,
}

impl HidppDeviceError {
    pub fn name(self) -> &'static str {
        match self.code {
            0x00 => "no error",
            0x01 => "unknown",
            0x02 => "invalid argument",
            0x03 => "out of range",
            0x04 => "hardware error",
            0x05 => "Logitech internal error",
            0x06 => "invalid feature index",
            0x07 => "invalid function ID",
            0x08 => "busy",
            0x09 => "unsupported",
            _ => "unrecognized error",
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReportKind(id) => write!(f, "unsupported HID++ report ID 0x{id:02x}"),
            Self::InvalidReportLength {
                kind,
                actual,
                expected,
            } => {
                write!(
                    f,
                    "report 0x{kind:02x} has length {actual}; expected {expected}"
                )
            }
            Self::InvalidFunction(value) => write!(f, "function ID {value} exceeds four bits"),
            Self::InvalidSoftwareId(value) => write!(f, "software ID {value} must be in 1..=15"),
            Self::ParameterTooLong {
                kind,
                actual,
                maximum,
            } => {
                write!(
                    f,
                    "report 0x{kind:02x} has {actual} parameter bytes; maximum is {maximum}"
                )
            }
            Self::MalformedPacket(reason) => write!(f, "malformed HID++ packet: {reason}"),
            Self::UnexpectedDeviceIndex { expected, actual } => {
                write!(
                    f,
                    "response device index 0x{actual:02x} does not match 0x{expected:02x}"
                )
            }
            Self::Device(error) => write!(f, "device error 0x{:02x}: {}", error.code, error.name()),
            Self::Timeout => f.write_str("timed out waiting for a matching HID++ response"),
            Self::FeatureSetTooLarge(count) => {
                write!(f, "FeatureSet count {count} exceeds safety limit")
            }
            Self::FeatureSetIndexMismatch {
                feature_id,
                table_index,
                root_index,
            } => write!(
                f,
                "feature 0x{feature_id:04x} is table index {table_index} but Root resolved index {root_index}"
            ),
            Self::MissingRequiredParameter {
                feature,
                function,
                required,
                actual,
            } => write!(
                f,
                "feature 0x{feature:04x} function 0x{function:x} needs {required} parameter bytes; received {actual}"
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug)]
pub enum ClientError {
    Transport(String),
    Protocol(ProtocolError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) => write!(f, "HID transport error: {message}"),
            Self::Protocol(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<ProtocolError> for ClientError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}
