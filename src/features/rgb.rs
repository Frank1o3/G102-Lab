//! HID++ 0x8071 RGB effects discovery and color-set requests; persistence is not established.
//! Feature indexes are resolved by Root discovery and passed by the caller.

use super::FeatureError;
use crate::protocol::HidppClient;

pub const FEATURE_ID: u16 = 0x8071;
const GET_INFO: u8 = 0;
const SET_CLUSTER_EFFECT: u8 = 1;
const MANAGE_SW_CONTROL: u8 = 5;
const ALL: u8 = 0xff;
const SETTER_PERSISTENCE_PARAMETER: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectSettings {
    Off,
    Static {
        color: [u8; 3],
    },
    Cycle {
        period_ms: u16,
        brightness: u8,
    },
    Wave {
        period_ms: u16,
        direction: WaveDirection,
    },
    Breathing {
        color: [u8; 3],
        period_ms: u16,
        brightness: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveDirection {
    Right,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingInfo {
    pub cluster_count: u8,
    /// Raw device-reported NV capability field; this is not evidence that a
    /// particular setting was persisted.
    pub nv_capabilities: u16,
    /// Raw device-reported extended capability field; meanings are not
    /// interpreted beyond the queried response.
    pub extended_capabilities: u16,
    /// Software-control/event status bytes returned by the device query.
    pub software_control: (u8, u8),
    pub clusters: Vec<LightingCluster>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingCluster {
    pub index: u8,
    pub location: u16,
    /// Raw device-reported display persistence capability byte.
    pub persistence_capabilities: u8,
    /// Whether the device reports effect persistence support; not whether the
    /// effect set by this application actually persisted.
    pub effect_persistence_supported: bool,
    pub multi_led_pattern_supported: bool,
    pub effects: Vec<LightingEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingEffect {
    /// Device-reported effect ID. This is distinct from the local slot used by setters.
    pub id: u16,
    pub local_slot: u8,
    pub capabilities: u16,
    pub period: u16,
}

fn malformed(why: &'static str) -> FeatureError {
    FeatureError::Malformed(why)
}

pub fn inspect<T: crate::hid::HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
) -> Result<LightingInfo, FeatureError> {
    let device = client.request(index, GET_INFO, &[ALL, ALL, 0])?;
    if device.params.len() < 7 {
        return Err(malformed("short RGB device-info response"));
    }
    let count = device.params[2];
    // Reject unreasonable counts rather than trusting a corrupt response to allocate or loop indefinitely.
    if count > 32 {
        return Err(malformed("RGB cluster count exceeds safety limit"));
    }
    let software_reply = client.request(index, MANAGE_SW_CONTROL, &[0, 0, 0])?;
    if software_reply.params.len() < 3 {
        return Err(malformed("short RGB software-control response"));
    }
    let software_control = (software_reply.params[1], software_reply.params[2]);
    let mut clusters = Vec::with_capacity(count as usize);
    for cluster in 0..count {
        let reply = client.request(index, GET_INFO, &[cluster, ALL, 0])?;
        if reply.params.len() < 8 {
            return Err(malformed("short RGB cluster-info response"));
        }
        let effects_count = reply.params[4];
        if effects_count > 64 {
            return Err(malformed("RGB effect count exceeds safety limit"));
        }
        let mut effects = Vec::with_capacity(effects_count as usize);
        for slot in 0..effects_count {
            let effect = client.request(index, GET_INFO, &[cluster, slot, 0])?;
            if effect.params.len() < 8 {
                return Err(malformed("short RGB effect-info response"));
            }
            effects.push(LightingEffect {
                id: u16::from_be_bytes([effect.params[2], effect.params[3]]),
                local_slot: slot,
                capabilities: u16::from_be_bytes([effect.params[4], effect.params[5]]),
                period: u16::from_be_bytes([effect.params[6], effect.params[7]]),
            });
        }
        clusters.push(LightingCluster {
            index: cluster,
            location: u16::from_be_bytes([reply.params[2], reply.params[3]]),
            persistence_capabilities: reply.params[5],
            effect_persistence_supported: reply.params[6] != 0,
            multi_led_pattern_supported: reply.params[7] != 0,
            effects,
        });
    }
    Ok(LightingInfo {
        cluster_count: count,
        nv_capabilities: u16::from_be_bytes([device.params[3], device.params[4]]),
        extended_capabilities: u16::from_be_bytes([device.params[5], device.params[6]]),
        software_control,
        clusters,
    })
}

/// Send the G102/G203-reference software-control sequence and set a
/// device-reported effect slot. The setter's final parameter is `0x01`, which
/// references describe as volatile; actual persistence behavior on the target
/// is not established. This function sends no 0x8100 onboard-profile requests.
pub fn set_effect<T: crate::hid::HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
    cluster_index: u8,
    effect_index: u8,
    settings: EffectSettings,
) -> Result<(), FeatureError> {
    let info = inspect(client, index)?;
    let cluster = info
        .clusters
        .get(cluster_index as usize)
        .filter(|c| c.index == cluster_index)
        .ok_or_else(|| malformed("cluster index was not reported by device"))?;
    let effect = cluster
        .effects
        .iter()
        .find(|effect| effect.local_slot == effect_index)
        .ok_or(
            FeatureError::Malformed("effect index was not reported for this cluster; run `rgb effects` to list valid indexes"),
        )?;
    let effect_params = encode_effect(effect_index, effect.id, settings)?;
    // The reference sequence requests software control. This does not by itself
    // establish display, effect, or onboard-flash persistence behavior.
    client.request(index, MANAGE_SW_CONTROL, &[0x01, 0x03, 0x05])?;
    let mut params = [0u8; 13];
    params[0] = cluster_index;
    params[1] = effect.local_slot;
    params[2..12].copy_from_slice(&effect_params);
    params[12] = SETTER_PERSISTENCE_PARAMETER;
    client.request(index, SET_CLUSTER_EFFECT, &params)?;
    Ok(())
}

fn encode_effect(
    effect_index: u8,
    effect_id: u16,
    settings: EffectSettings,
) -> Result<[u8; 10], FeatureError> {
    let mut effect_params = [0u8; 10];
    match (effect_id, settings) {
        (0x0000, EffectSettings::Off) => {}
        (0x0001, EffectSettings::Static { color }) => effect_params[..3].copy_from_slice(&color),
        (
            0x0003,
            EffectSettings::Cycle {
                period_ms,
                brightness,
            },
        ) => {
            validate_effect_values(period_ms, brightness)?;
            effect_params[5..7].copy_from_slice(&period_ms.to_be_bytes());
            effect_params[7] = brightness;
        }
        (
            0x0004,
            EffectSettings::Wave {
                period_ms,
                direction,
            },
        ) => {
            validate_period(period_ms)?;
            effect_params[6..8].copy_from_slice(&period_ms.to_be_bytes());
            effect_params[8] = match direction {
                WaveDirection::Right => 0x01,
                WaveDirection::Left => 0x06,
            };
        }
        (
            0x000a,
            EffectSettings::Breathing {
                color,
                period_ms,
                brightness,
            },
        ) => {
            validate_effect_values(period_ms, brightness)?;
            effect_params[..3].copy_from_slice(&color);
            effect_params[3..5].copy_from_slice(&period_ms.to_be_bytes());
            effect_params[6] = brightness;
        }
        (id, _) => {
            return Err(FeatureError::UnsupportedRgbEffect {
                index: effect_index,
                id,
            });
        }
    }
    Ok(effect_params)
}

fn validate_period(period_ms: u16) -> Result<(), FeatureError> {
    if !(1000..=8000).contains(&period_ms) {
        return Err(FeatureError::Malformed(
            "effect period must be 1000..=8000 ms",
        ));
    }
    Ok(())
}

fn validate_effect_values(period_ms: u16, brightness: u8) -> Result<(), FeatureError> {
    validate_period(period_ms)?;
    if !(1..=100).contains(&brightness) {
        return Err(FeatureError::Malformed("effect brightness must be 1..=100"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hid::HidTransport,
        protocol::{Packet, ReportKind},
    };
    use std::{collections::VecDeque, time::Duration};

    struct Mock {
        replies: VecDeque<Vec<u8>>,
        writes: Vec<Vec<u8>>,
    }
    impl HidTransport for Mock {
        fn write_report(&mut self, report: &[u8]) -> Result<(), String> {
            self.writes.push(report.to_vec());
            Ok(())
        }
        fn read_report(&mut self, _: Duration) -> Result<Option<Vec<u8>>, String> {
            Ok(self.replies.pop_front())
        }
    }
    fn reply(function: u8, software_id: u8, params: &[u8]) -> Vec<u8> {
        Packet::new(ReportKind::Long, 0xff, 0x0e, function, software_id, params)
            .unwrap()
            .encode()
    }
    fn mock() -> Mock {
        let mut replies = Vec::new();
        let mut software_id = 0x0b;
        append_scan(&mut replies, &mut software_id);
        replies.push(reply(5, software_id, &[]));
        software_id = if software_id == 0x0f {
            1
        } else {
            software_id + 1
        };
        replies.push(reply(1, software_id, &[]));
        Mock {
            replies: replies.into(),
            writes: vec![],
        }
    }

    fn append_scan(replies: &mut Vec<Vec<u8>>, software_id: &mut u8) {
        replies.push(reply(0, *software_id, &[0xff, 0xff, 1, 0, 3, 0, 0]));
        bump(software_id);
        replies.push(reply(5, *software_id, &[0, 0, 0]));
        bump(software_id);
        replies.push(reply(0, *software_id, &[0, 0xff, 0, 2, 1, 1, 0, 0]));
        bump(software_id);
        replies.push(reply(0, *software_id, &[0, 0, 0, 1, 0, 1, 0, 0]));
        bump(software_id);
    }

    fn bump(id: &mut u8) {
        *id = if *id == 0x0f { 1 } else { *id + 1 };
    }

    #[test]
    fn discovers_cluster_effect_ids_and_reported_persistence_capabilities() {
        let mut client = HidppClient::new(mock(), 0xff, ReportKind::Long);
        let info = inspect(&mut client, 0x0e).unwrap();
        assert_eq!(info.cluster_count, 1);
        assert_eq!(info.nv_capabilities, 3);
        assert_eq!(info.clusters[0].location, 2);
        assert_eq!(info.software_control, (0, 0));
        assert_eq!(info.clusters[0].persistence_capabilities, 1);
        assert_eq!(info.clusters[0].effects[0].id, 1);
    }

    #[test]
    fn static_setter_claims_software_and_sends_reference_persistence_parameter() {
        let mut client = HidppClient::new(mock(), 0xff, ReportKind::Long);
        set_effect(
            &mut client,
            0x0e,
            0,
            0,
            EffectSettings::Static {
                color: [0x12, 0x34, 0x56],
            },
        )
        .unwrap();
        let transport = client.into_transport();
        let setter = transport.writes.last().unwrap();
        assert_eq!(setter[2], 0x0e);
        assert_eq!(setter[3] >> 4, SET_CLUSTER_EFFECT);
        assert_eq!(&setter[4..9], &[0, 0, 0x12, 0x34, 0x56]);
        assert_eq!(setter[16], SETTER_PERSISTENCE_PARAMETER);
        assert!(
            transport
                .writes
                .iter()
                .any(|report| report[3] >> 4 == MANAGE_SW_CONTROL && report[4..7] == [1, 3, 5])
        );
    }

    #[test]
    fn g102_effect_payloads_follow_the_documented_layouts() {
        assert_eq!(
            encode_effect(
                1,
                1,
                EffectSettings::Static {
                    color: [0x11, 0x22, 0x33]
                }
            )
            .unwrap()[..4],
            [0x11, 0x22, 0x33, 0]
        );
        assert_eq!(
            encode_effect(
                2,
                3,
                EffectSettings::Cycle {
                    period_ms: 3000,
                    brightness: 80
                }
            )
            .unwrap(),
            [0, 0, 0, 0, 0, 0x0b, 0xb8, 80, 0, 0]
        );
        assert_eq!(
            encode_effect(
                3,
                4,
                EffectSettings::Wave {
                    period_ms: 4000,
                    direction: WaveDirection::Left
                }
            )
            .unwrap(),
            [0, 0, 0, 0, 0, 0, 0x0f, 0xa0, 0x06, 0]
        );
        assert_eq!(
            encode_effect(
                4,
                0x0a,
                EffectSettings::Breathing {
                    color: [0xaa, 0xbb, 0xcc],
                    period_ms: 5000,
                    brightness: 70
                }
            )
            .unwrap(),
            [0xaa, 0xbb, 0xcc, 0x13, 0x88, 0, 70, 0, 0, 0]
        );
        assert!(encode_effect(5, 0x0d, EffectSettings::Off).is_err());
    }
}
