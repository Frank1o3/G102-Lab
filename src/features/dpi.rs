//! HID++ 0x2201 Adjustable DPI (feature functions 0..3).

use crate::protocol::ReportKind;
use crate::{features::FeatureError, hid::HidTransport, protocol::HidppClient};

pub const FEATURE_ID: u16 = 0x2201;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiSupport {
    Discrete(Vec<u16>),
    Range {
        minimum: u16,
        maximum: u16,
        step: u16,
    },
}

impl DpiSupport {
    pub fn supports(&self, value: u16) -> bool {
        match self {
            Self::Discrete(values) => values.contains(&value),
            Self::Range {
                minimum,
                maximum,
                step,
            } => {
                *step != 0
                    && value >= *minimum
                    && value <= *maximum
                    && (value - minimum).is_multiple_of(*step)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiSensor {
    pub index: u8,
    pub current_dpi: u16,
    pub default_dpi: u16,
    pub support: DpiSupport,
}

impl DpiSensor {
    pub fn supports(&self, value: u16) -> bool {
        self.support.supports(value)
    }
}

pub fn read_sensor<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    sensor: u8,
) -> Result<DpiSensor, FeatureError> {
    let count = client.request(feature_index, 0x0, &[])?.params[0];
    if sensor >= count {
        return Err(FeatureError::InvalidSensor {
            requested: sensor,
            count,
        });
    }

    // A short report carries only three parameter bytes, which cannot hold a
    // complete supported-value list and range marker. Do not infer that a
    // truncated list is the device's complete capability set.
    if client.report_kind == ReportKind::Short {
        return Err(FeatureError::InsufficientReportCapacity {
            feature: crate::protocol::FeatureId(FEATURE_ID),
            report_id: client.report_kind.id(),
        });
    }

    let list = client.request(feature_index, 0x1, &[sensor])?;
    require_echo(sensor, &list.params)?;
    // Function 1 returns sensor index followed by up to seven big-endian u16
    // values in the long response. 0xe000+step marks a stepped range.
    if list.params.len() < 3 {
        return Err(FeatureError::Malformed("DPI list lacks a value"));
    }
    let mut values = Vec::new();
    let mut step = None;
    for pair in list.params[1..].as_chunks::<2>().0 {
        let value = u16::from_be_bytes([pair[0], pair[1]]);
        if value == 0 {
            break;
        }
        if value > 0xe000 {
            step = Some(value - 0xe000);
        } else {
            values.push(value);
        }
    }
    let support = if let Some(step) = step {
        if step == 0 || values.len() < 2 {
            return Err(FeatureError::Malformed("invalid DPI range encoding"));
        }
        let minimum = *values.iter().min().unwrap();
        let maximum = *values.iter().max().unwrap();
        DpiSupport::Range {
            minimum,
            maximum,
            step,
        }
    } else if !values.is_empty() {
        DpiSupport::Discrete(values)
    } else {
        return Err(FeatureError::Malformed("empty DPI support list"));
    };

    let current = client.request(feature_index, 0x2, &[sensor])?;
    require_echo(sensor, &current.params)?;
    if current.params.len() < 5 {
        return Err(FeatureError::Malformed(
            "DPI current/default response is truncated",
        ));
    }
    Ok(DpiSensor {
        index: sensor,
        current_dpi: u16::from_be_bytes([current.params[1], current.params[2]]),
        default_dpi: u16::from_be_bytes([current.params[3], current.params[4]]),
        support,
    })
}

pub fn read_all<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
) -> Result<Vec<DpiSensor>, FeatureError> {
    let count = client.request(feature_index, 0x0, &[])?.params[0];
    if count == 0 {
        return Err(FeatureError::Malformed("device reports zero DPI sensors"));
    }
    (0..count)
        .map(|sensor| read_sensor(client, feature_index, sensor))
        .collect()
}

pub fn set_dpi<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    sensor: u8,
    dpi: u16,
) -> Result<DpiSensor, FeatureError> {
    let before = read_sensor(client, feature_index, sensor)?;
    if !before.supports(dpi) {
        return Err(FeatureError::UnsupportedDpi { requested: dpi });
    }
    client.request(feature_index, 0x3, &[sensor, (dpi >> 8) as u8, dpi as u8])?;
    let after = read_sensor(client, feature_index, sensor)?;
    if after.current_dpi != dpi {
        return Err(FeatureError::VerificationMismatch {
            setting: "DPI",
            requested: dpi,
            actual: after.current_dpi,
        });
    }
    Ok(after)
}

fn require_echo(sensor: u8, params: &[u8]) -> Result<(), FeatureError> {
    if params.is_empty() {
        return Err(FeatureError::Malformed("DPI response lacks sensor index"));
    }
    if params[0] != sensor {
        return Err(FeatureError::Malformed(
            "DPI response sensor index does not match request",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hid::HidTransport,
        protocol::{HidppClient, Packet, ReportKind},
    };
    use std::{collections::VecDeque, time::Duration};

    #[derive(Default)]
    struct Mock {
        replies: VecDeque<Vec<u8>>,
        writes: Vec<Vec<u8>>,
    }
    impl HidTransport for Mock {
        fn write_report(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.writes.push(bytes.to_vec());
            Ok(())
        }
        fn read_report(&mut self, _: Duration) -> Result<Option<Vec<u8>>, String> {
            Ok(self.replies.pop_front())
        }
    }
    fn reply(fn_id: u8, sw: u8, params: &[u8]) -> Vec<u8> {
        Packet::new(ReportKind::Long, 0xff, 7, fn_id, sw, params)
            .unwrap()
            .encode()
    }
    fn client(replies: Vec<Vec<u8>>) -> HidppClient<Mock> {
        HidppClient::new(
            Mock {
                replies: replies.into(),
                writes: vec![],
            },
            0xff,
            ReportKind::Long,
        )
    }

    #[test]
    fn parses_range_and_current_default() {
        let mut c = client(vec![
            reply(0, 0x0b, &[1]),
            reply(1, 0x0c, &[0, 0x01, 0x2c, 0xe0, 0x0a, 0x1f, 0x40]),
            reply(2, 0x0d, &[0, 0x07, 0xd0, 0x03, 0xe8]),
        ]);
        let sensor = read_sensor(&mut c, 7, 0).unwrap();
        assert_eq!(
            sensor.support,
            DpiSupport::Range {
                minimum: 300,
                maximum: 8000,
                step: 10
            }
        );
        assert_eq!((sensor.current_dpi, sensor.default_dpi), (2000, 1000));
        assert!(sensor.supports(3010));
        assert!(!sensor.supports(3001));
    }

    #[test]
    fn rejects_unsupported_value_without_set_request() {
        let mut c = client(vec![
            reply(0, 0x0b, &[1]),
            reply(1, 0x0c, &[0, 0x01, 0x2c, 0xe0, 0x0a, 0x1f, 0x40]),
            reply(2, 0x0d, &[0, 0x07, 0xd0, 0x03, 0xe8]),
        ]);
        assert!(matches!(
            set_dpi(&mut c, 7, 0, 3001),
            Err(FeatureError::UnsupportedDpi { .. })
        ));
        assert_eq!(c.into_transport().writes.len(), 3);
    }

    #[test]
    fn sets_supported_value_and_reads_it_back() {
        let mut c = client(vec![
            reply(0, 0x0b, &[1]),
            reply(1, 0x0c, &[0, 0x01, 0x2c, 0xe0, 0x0a, 0x1f, 0x40]),
            reply(2, 0x0d, &[0, 0x07, 0xd0, 0x03, 0xe8]),
            reply(3, 0x0e, &[]),
            reply(0, 0x0f, &[1]),
            reply(1, 0x01, &[0, 0x01, 0x2c, 0xe0, 0x0a, 0x1f, 0x40]),
            reply(2, 0x02, &[0, 0x0b, 0xb8, 0x03, 0xe8]),
        ]);
        let sensor = set_dpi(&mut c, 7, 0, 3000).unwrap();
        assert_eq!(sensor.current_dpi, 3000);
        assert_eq!(c.into_transport().writes.len(), 7);
    }

    #[test]
    fn refuses_to_infer_support_from_short_truncated_list() {
        let mut c = HidppClient::new(
            Mock {
                replies: vec![Packet::new(ReportKind::Short, 0xff, 7, 0, 0x0b, &[1])
                    .unwrap()
                    .encode()]
                .into(),
                writes: vec![],
            },
            0xff,
            ReportKind::Short,
        );
        assert!(matches!(
            read_sensor(&mut c, 7, 0),
            Err(FeatureError::InsufficientReportCapacity { .. })
        ));
        assert_eq!(c.into_transport().writes.len(), 1);
    }
}
