//! HID++ 0x8060 Adjustable Report Rate. Rates are advertised as intervals.

use crate::{features::FeatureError, hid::HidTransport, protocol::HidppClient};

pub const FEATURE_ID: u16 = 0x8060;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportRate {
    pub interval_ms: u8,
    pub hz: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRateInfo {
    pub supported: Vec<ReportRate>,
    pub current: ReportRate,
}

pub fn read<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
) -> Result<ReportRateInfo, FeatureError> {
    let bitmap = client.request(feature_index, 0x0, &[])?.params[0];
    let supported: Vec<_> = (0..8)
        .filter(|bit| bitmap & (1 << bit) != 0)
        .map(|bit| {
            let ms = bit + 1;
            ReportRate {
                interval_ms: ms,
                hz: 1000 / ms as u16,
            }
        })
        .collect();
    if supported.is_empty() {
        return Err(FeatureError::Malformed("empty report-rate bitmap"));
    }
    let response = client.request(feature_index, 0x1, &[])?;
    let current_ms = response.params[0];
    let current = supported
        .iter()
        .find(|rate| rate.interval_ms == current_ms)
        .copied()
        .ok_or(FeatureError::Malformed(
            "current interval is absent from supported bitmap",
        ))?;
    Ok(ReportRateInfo { supported, current })
}

pub fn set_hz<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    hz: u16,
) -> Result<ReportRateInfo, FeatureError> {
    let before = read(client, feature_index)?;
    let Some(rate) = before.supported.iter().find(|rate| rate.hz == hz).copied() else {
        return Err(FeatureError::UnsupportedRate {
            requested_hz: hz,
            supported_hz: before.supported.iter().map(|rate| rate.hz).collect(),
        });
    };
    client.request(feature_index, 0x2, &[rate.interval_ms])?;
    let after = read(client, feature_index)?;
    if after.current != rate {
        return Err(FeatureError::VerificationMismatch {
            setting: "Hz",
            requested: hz,
            actual: after.current.hz,
        });
    }
    Ok(after)
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
        Packet::new(ReportKind::Short, 0xff, 4, fn_id, sw, params)
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
            ReportKind::Short,
        )
    }

    #[test]
    fn parses_supported_periods_and_current_rate() {
        let mut c = client(vec![reply(0, 0x0b, &[0b0000_1011]), reply(1, 0x0c, &[2])]);
        let info = read(&mut c, 4).unwrap();
        assert_eq!(
            info.supported
                .iter()
                .map(|r| (r.interval_ms, r.hz))
                .collect::<Vec<_>>(),
            vec![(1, 1000), (2, 500), (4, 250)]
        );
        assert_eq!(info.current.hz, 500);
    }

    #[test]
    fn rejects_unadvertised_rate_without_set_request() {
        let mut c = client(vec![reply(0, 0x0b, &[0b0000_1011]), reply(1, 0x0c, &[2])]);
        assert!(matches!(
            set_hz(&mut c, 4, 333),
            Err(FeatureError::UnsupportedRate { .. })
        ));
        assert_eq!(c.into_transport().writes.len(), 2);
    }

    #[test]
    fn sets_and_reads_back_interval() {
        let mut c = client(vec![
            reply(0, 0x0b, &[0b0000_0011]),
            reply(1, 0x0c, &[2]),
            reply(2, 0x0d, &[]),
            reply(0, 0x0e, &[0b0000_0011]),
            reply(1, 0x0f, &[1]),
        ]);
        assert_eq!(set_hz(&mut c, 4, 1000).unwrap().current.interval_ms, 1);
    }
}
