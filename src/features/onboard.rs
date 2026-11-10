//! Read-only HID++ 0x8100 onboard-profile inspection.
//!
//! HID++ 0x8100 read-only inspection and offline G102 format-4 decoding.

use std::fmt;
use std::time::Duration;

use super::FeatureError;
use crate::{
    error::{ClientError, ProtocolError},
    hid::HidTransport,
    protocol::HidppClient,
};

pub const FEATURE_ID: u16 = 0x8100;
const FN_GET_INFO: u8 = 0;
const FN_GET_MODE: u8 = 2;
const FN_GET_CURRENT_PROFILE: u8 = 4;
const FN_MEMORY_READ: u8 = 5;
pub const FN_START_WRITE: u8 = 6;
pub const FN_WRITE_DATA: u8 = 7;
pub const FN_END_WRITE: u8 = 8;
const DIRECTORY_SECTOR: u16 = 0;
const ROM_DIRECTORY_SECTOR: u16 = 0x0100;
const READ_SIZE: usize = 16;
const MAX_SECTOR_SIZE: usize = 4096;
const MAX_TOTAL_MEMORY: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingMode {
    Onboard,
    Host,
    Unknown(u8),
}

impl OperatingMode {
    fn from_wire(value: u8) -> Self {
        match value {
            1 => Self::Onboard,
            2 => Self::Host,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardInfo {
    pub memory_model: u8,
    pub profile_format: u8,
    pub macro_format: u8,
    pub profile_count: u8,
    pub factory_profile_count: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub mechanical_layout: u8,
    pub various_info: u8,
    pub raw: [u8; 11],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub slot: usize,
    pub sector: u16,
    pub enabled: bool,
    pub raw: [u8; 4],
    pub valid_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumCandidate {
    Erased,
    Crc16CcittFalseMatch(u16),
    Crc16CcittFalseMismatch { stored: u16, calculated: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSector {
    pub id: u16,
    pub bytes: Vec<u8>,
    pub checksum_candidate: ChecksumCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardInspection {
    pub info: OnboardInfo,
    pub mode: OperatingMode,
    pub current_profile_raw: [u8; 3],
    pub current_profile: Option<u8>,
    pub user_directory: Vec<DirectoryEntry>,
    pub factory_directory: Vec<DirectoryEntry>,
    pub profiles: Vec<RawSector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBackup {
    pub info: OnboardInfo,
    pub mode: OperatingMode,
    pub current_profile_raw: [u8; 3],
    pub sectors: Vec<RawSector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonAction {
    MouseButton {
        mask: u16,
        label: Option<&'static str>,
    },
    Keyboard {
        modifiers: u8,
        key: u8,
    },
    Consumer {
        usage: u16,
    },
    Special {
        code: u8,
        label: Option<&'static str>,
        parameters: [u8; 2],
    },
    Macro {
        sector: u16,
        address: u16,
    },
    Disabled,
    Unknown([u8; 4]),
}

/// Standard names for recognized mouse-mask encodings. The labels do not
/// establish which physical G102 button corresponds to a profile slot.
pub fn mouse_button_label(mask: u16) -> Option<&'static str> {
    match mask {
        0x0001 => Some("Left"),
        0x0002 => Some("Right"),
        0x0004 => Some("Middle"),
        0x0008 => Some("Back"),
        0x0010 => Some("Forward"),
        0x0020 => Some("Button 6"),
        0x0040 => Some("Button 7"),
        0x0080 => Some("Button 8"),
        _ => None,
    }
}

pub fn special_code_label(code: u8) -> Option<&'static str> {
    match code {
        0x00 => Some("No Op"),
        0x01 => Some("Tilt Left"),
        0x02 => Some("Tilt Right"),
        0x03 => Some("Next DPI"),
        0x04 => Some("Previous DPI"),
        0x05 => Some("Cycle DPI (reference label; behavior unverified)"),
        0x06 => Some("Default DPI"),
        0x07 => Some("Shift DPI"),
        0x08 => Some("Next Profile"),
        0x09 => Some("Previous Profile"),
        0x0A => Some("Cycle Profile"),
        0x0B => Some("G-Shift"),
        0x0C => Some("Battery Status"),
        0x0D => Some("Profile Select"),
        0x0E => Some("Mode Switch"),
        0x0F => Some("Host Button"),
        0x10 => Some("Scroll Down"),
        0x11 => Some("Scroll Up"),
        _ => None,
    }
}

impl ButtonAction {
    pub fn from_bytes(raw: [u8; 4]) -> Self {
        match raw {
            [0xff, 0xff, 0xff, 0xff] => Self::Disabled,
            [0x00, 0x00, 0x00, 0x00] => Self::Disabled,
            [0x80, 0x01, hi, lo] => {
                let mask = u16::from_be_bytes([hi, lo]);
                let label = mouse_button_label(mask);
                Self::MouseButton { mask, label }
            }
            [0x80, 0x02, modifiers, key] => Self::Keyboard { modifiers, key },
            [0x80, 0x03, hi, lo] => {
                let usage = u16::from_be_bytes([hi, lo]);
                Self::Consumer { usage }
            }
            [0x90, code, p0, p1] => {
                let label = special_code_label(code);
                Self::Special {
                    code,
                    label,
                    parameters: [p0, p1],
                }
            }
            [b0, b1, b2, b3] if (b0 >> 4) <= 2 && raw != [0, 0, 0, 0] => {
                let sector = (((b0 & 0x0f) as u16) << 8) | (b1 as u16);
                let address = u16::from_be_bytes([b2, b3]);
                Self::Macro { sector, address }
            }
            _ => Self::Unknown(raw),
        }
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        match self {
            Self::Disabled => [0xff, 0xff, 0xff, 0xff],
            Self::MouseButton { mask, .. } => {
                let [hi, lo] = mask.to_be_bytes();
                [0x80, 0x01, hi, lo]
            }
            Self::Keyboard { modifiers, key } => [0x80, 0x02, *modifiers, *key],
            Self::Consumer { usage } => {
                let [hi, lo] = usage.to_be_bytes();
                [0x80, 0x03, hi, lo]
            }
            Self::Special {
                code, parameters, ..
            } => [0x90, *code, parameters[0], parameters[1]],
            Self::Macro { sector, address } => {
                let sh = ((sector >> 8) & 0x0f) as u8;
                let sl = (sector & 0xff) as u8;
                let [ah, al] = address.to_be_bytes();
                [sh, sl, ah, al]
            }
            Self::Unknown(raw) => *raw,
        }
    }

    /// Returns whether this four-byte encoding is represented in the supplied
    /// G102 backup or accepted by the evidence-scoped offline editor. This does
    /// not verify behavior or map the stored slot to a physical button.
    pub fn is_corroborated_g102_encoding(&self) -> bool {
        match self {
            Self::MouseButton { mask, .. } => matches!(*mask, 1 | 2 | 4 | 8 | 16),
            Self::Special {
                code: 0x05,
                parameters: [0xff, 0xff],
                ..
            } => true,
            Self::Disabled => true,
            _ => false,
        }
    }

    pub fn display_description(&self) -> String {
        match self {
            Self::MouseButton { mask, label } => {
                format!(
                    "mouse mask 0x{mask:04x}{}",
                    label.map(|text| format!(" ({text})")).unwrap_or_default()
                )
            }
            Self::Keyboard { modifiers, key } => {
                format!(
                    "keyboard key 0x{key:02x} (modifiers 0x{modifiers:02x}) [unverified on G102]"
                )
            }
            Self::Consumer { usage } => {
                format!("consumer control usage 0x{usage:04x} [unverified on G102]")
            }
            Self::Special {
                code,
                label,
                parameters,
            } => {
                format!(
                    "special 0x{code:02x}{} params {:02x?}",
                    label.map(|text| format!(" ({text})")).unwrap_or_default(),
                    parameters
                )
            }
            Self::Macro { sector, address } => {
                format!("macro sector 0x{sector:04x} addr 0x{address:04x} [unverified on G102]")
            }
            Self::Disabled => "disabled (all-ff record)".into(),
            Self::Unknown(raw) => format!("unrecognized {raw:02x?}; raw bytes preserved"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Encodings accepted by the offline profile editor. The legacy type name does
/// not imply that physical action behavior or button positions are verified.
pub enum VerifiedAction {
    LeftClick,
    RightClick,
    MiddleClick,
    Back,
    Forward,
    DpiCycle,
    Disabled,
}

impl std::str::FromStr for VerifiedAction {
    type Err = FeatureError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let clean = s.trim().to_ascii_lowercase();
        match clean.as_str() {
            "left" | "click-left" | "left-click" | "leftclick" => Ok(Self::LeftClick),
            "right" | "click-right" | "right-click" | "rightclick" => Ok(Self::RightClick),
            "middle" | "click-middle" | "middle-click" | "middleclick" => Ok(Self::MiddleClick),
            "back" => Ok(Self::Back),
            "forward" => Ok(Self::Forward),
            "dpi-cycle" | "cycle-dpi" | "dpicycle" | "cycledpi" => Ok(Self::DpiCycle),
            "disable" | "disabled" | "none" | "off" => Ok(Self::Disabled),
            other => {
                if other.starts_with("key") || other.starts_with("nav-") || other.contains("cmd") {
                    Err(FeatureError::Malformed(
                        "keyboard key actions are unverified on G102 format-4 onboard profiles and cannot be safely set",
                    ))
                } else if other.starts_with("consumer") || other.starts_with("ac-") {
                    Err(FeatureError::Malformed(
                        "consumer control actions are unverified on G102 format-4 onboard profiles and cannot be safely set",
                    ))
                } else if other.starts_with("macro") {
                    Err(FeatureError::Malformed(
                        "macro actions are unverified on G102 format-4 onboard profiles and cannot be safely set",
                    ))
                } else if other.starts_with("dpi-")
                    || other.starts_with("profile-")
                    || other.contains("shift")
                {
                    Err(FeatureError::Malformed(
                        "special function actions other than dpi-cycle have unverified parameter layouts on G102 and cannot be safely set",
                    ))
                } else {
                    Err(FeatureError::Malformed(
                        "unsupported or unverified action; accepted offline encodings are: left, right, middle, back, forward, dpi-cycle, disable",
                    ))
                }
            }
        }
    }
}

impl VerifiedAction {
    pub fn parse(s: &str) -> Result<Self, FeatureError> {
        use std::str::FromStr;
        Self::from_str(s)
    }

    pub fn to_bytes(self) -> [u8; 4] {
        match self {
            Self::LeftClick => [0x80, 0x01, 0x00, 0x01],
            Self::RightClick => [0x80, 0x01, 0x00, 0x02],
            Self::MiddleClick => [0x80, 0x01, 0x00, 0x04],
            Self::Back => [0x80, 0x01, 0x00, 0x08],
            Self::Forward => [0x80, 0x01, 0x00, 0x10],
            Self::DpiCycle => [0x90, 0x05, 0xff, 0xff],
            Self::Disabled => [0xff, 0xff, 0xff, 0xff],
        }
    }

    pub fn to_button_action(self) -> ButtonAction {
        ButtonAction::from_bytes(self.to_bytes())
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::LeftClick => "Left Click (mouse mask 0x0001)",
            Self::RightClick => "Right Click (mouse mask 0x0002)",
            Self::MiddleClick => "Middle Click (mouse mask 0x0004)",
            Self::Back => "Back (mouse mask 0x0008)",
            Self::Forward => "Forward (mouse mask 0x0010)",
            Self::DpiCycle => "DPI Cycle (special 0x05 params [0xff, 0xff])",
            Self::Disabled => "Disabled ([0xff, 0xff, 0xff, 0xff])",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonChange {
    pub slot: usize,
    pub offset: usize,
    pub old_raw: [u8; 4],
    pub new_raw: [u8; 4],
    pub old_action: ButtonAction,
    pub new_action: ButtonAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteDiff {
    pub offset: usize,
    pub old_byte: u8,
    pub new_byte: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEditPlan {
    pub profile_sector_id: u16,
    pub changes: Vec<ButtonChange>,
    pub old_checksum: u16,
    pub new_checksum: u16,
    pub byte_diffs: Vec<ByteDiff>,
    pub modified_backup: RawBackup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupDiff {
    pub sector_diffs: Vec<(u16, Vec<ByteDiff>)>,
    pub metadata_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedButton {
    pub slot: usize,
    pub raw: [u8; 4],
    pub action: ButtonAction,
}

/// Values are retained with their source bytes. Their offsets/meanings are the
/// recovered base-v1 schema used by format 4, corroborated (not behaviorally
/// validated) by this G102 backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedG102Profile {
    pub sector_id: u16,
    pub report_rate_raw: u8,
    pub default_dpi_stage: u8,
    pub shifted_dpi_stage: u8,
    pub dpi_stages: [u16; 5],
    pub color_raw: [u8; 3],
    pub power_raw: u8,
    pub angle_snap_raw: u8,
    pub buttons: Vec<DecodedButton>,
    pub checksum: u16,
}

fn malformed(reason: &'static str) -> FeatureError {
    FeatureError::Malformed(reason)
}

pub fn read_info<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
) -> Result<OnboardInfo, FeatureError> {
    let descriptor = client.request(index, FN_GET_INFO, &[])?;
    if descriptor.params.len() < 11 {
        return Err(malformed("short onboard profile descriptor"));
    }
    let mut raw = [0; 11];
    raw.copy_from_slice(&descriptor.params[..11]);
    Ok(OnboardInfo {
        memory_model: raw[0],
        profile_format: raw[1],
        macro_format: raw[2],
        profile_count: raw[3],
        factory_profile_count: raw[4],
        button_count: raw[5],
        sector_count: raw[6],
        sector_size: u16::from_be_bytes([raw[7], raw[8]]),
        mechanical_layout: raw[9],
        various_info: raw[10],
        raw,
    })
}

pub fn read_mode<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
) -> Result<OperatingMode, FeatureError> {
    let mode_reply = client.request(index, FN_GET_MODE, &[])?;
    if mode_reply.params.is_empty() {
        return Err(malformed("short onboard mode response"));
    }
    Ok(OperatingMode::from_wire(mode_reply.params[0]))
}

pub fn read_current_profile_raw<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
) -> Result<[u8; 3], FeatureError> {
    let current_reply = client.request(index, FN_GET_CURRENT_PROFILE, &[])?;
    if current_reply.params.len() < 3 {
        return Err(malformed("short onboard current-profile response"));
    }
    Ok([
        current_reply.params[0],
        current_reply.params[1],
        current_reply.params[2],
    ])
}

pub fn inspect<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
) -> Result<OnboardInspection, FeatureError> {
    let info = read_info(client, index)?;
    validate_descriptor(&info)?;
    let mode = read_mode(client, index)?;
    let current_profile_raw = read_current_profile_raw(client, index)?;

    let user_dir_sector = read_sector(client, index, DIRECTORY_SECTOR, info.sector_size as usize)?;
    let user_directory = parse_directory(
        &user_dir_sector.bytes,
        info.profile_count,
        info.sector_count,
        false,
    );
    let factory_directory = if info.factory_profile_count > 0 {
        let raw = read_sector(
            client,
            index,
            ROM_DIRECTORY_SECTOR,
            info.sector_size as usize,
        )?;
        parse_directory(
            &raw.bytes,
            info.factory_profile_count,
            info.sector_count,
            true,
        )
    } else {
        Vec::new()
    };
    let mut profiles = Vec::new();
    for entry in user_directory
        .iter()
        .chain(factory_directory.iter())
        .filter(|entry| entry.valid_target)
    {
        profiles.push(read_sector(
            client,
            index,
            entry.sector,
            info.sector_size as usize,
        )?);
    }
    Ok(OnboardInspection {
        info,
        mode,
        current_profile: (current_profile_raw[1] != 0).then_some(current_profile_raw[1]),
        current_profile_raw,
        user_directory,
        factory_directory,
        profiles,
    })
}

pub fn validate_descriptor(info: &OnboardInfo) -> Result<(), FeatureError> {
    let size = info.sector_size as usize;
    if info.sector_count == 0 || !(READ_SIZE..=MAX_SECTOR_SIZE).contains(&size) {
        return Err(malformed(
            "onboard descriptor has unsupported sector count or size",
        ));
    }
    let total = (info.sector_count as usize)
        .checked_mul(size)
        .ok_or_else(|| malformed("onboard memory size overflow"))?;
    if total > MAX_TOTAL_MEMORY {
        return Err(malformed("onboard memory exceeds 4 MiB safety limit"));
    }
    if info.profile_count > 64 || info.factory_profile_count > 64 || info.button_count > 64 {
        return Err(malformed(
            "onboard descriptor exceeds profile/button safety limits",
        ));
    }
    Ok(())
}

pub fn read_sector<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
    sector: u16,
    size: usize,
) -> Result<RawSector, FeatureError> {
    if !(READ_SIZE..=MAX_SECTOR_SIZE).contains(&size) {
        return Err(malformed("invalid sector size for read"));
    }
    let mut bytes = vec![0; size];
    for offset in read_offsets(size) {
        let [sh, sl] = sector.to_be_bytes();
        let offset =
            u16::try_from(offset).map_err(|_| malformed("sector offset exceeds protocol range"))?;
        let [oh, ol] = offset.to_be_bytes();
        let reply = client.request(index, FN_MEMORY_READ, &[sh, sl, oh, ol])?;
        if reply.params.len() < READ_SIZE {
            return Err(malformed("short onboard memory-read response"));
        }
        let end = offset as usize + READ_SIZE;
        bytes[offset as usize..end].copy_from_slice(&reply.params[..READ_SIZE]);
    }
    let checksum_candidate = checksum_candidate(&bytes);
    Ok(RawSector {
        id: sector,
        bytes,
        checksum_candidate,
    })
}

pub fn read_offsets(size: usize) -> Vec<usize> {
    if size < READ_SIZE {
        return Vec::new();
    }
    let mut offsets: Vec<usize> = (0..=size - READ_SIZE).step_by(READ_SIZE).collect();
    if offsets.last().copied().map(|offset| offset + READ_SIZE) != Some(size) {
        offsets.push(size - READ_SIZE);
    }
    offsets
}

fn parse_directory(
    bytes: &[u8],
    max_entries: u8,
    writable_sectors: u8,
    factory: bool,
) -> Vec<DirectoryEntry> {
    let mut entries = Vec::new();
    for (slot, chunk) in bytes
        .as_chunks::<4>()
        .0
        .iter()
        .take(max_entries as usize)
        .enumerate()
    {
        let raw = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let sector = u16::from_be_bytes([chunk[0], chunk[1]]);
        if sector == 0xffff {
            break;
        }
        if sector == 0 {
            continue;
        }
        let valid_target = if factory {
            sector >= 0x0100 && sector < 0x0100 + max_entries as u16
        } else {
            sector < writable_sectors as u16
        };
        entries.push(DirectoryEntry {
            slot: slot + 1,
            sector,
            enabled: chunk[2] != 0,
            raw,
            valid_target,
        });
    }
    entries
}

pub fn checksum_candidate(bytes: &[u8]) -> ChecksumCandidate {
    if bytes.iter().all(|byte| *byte == 0xff) {
        return ChecksumCandidate::Erased;
    }
    if bytes.len() < 3 {
        return ChecksumCandidate::Crc16CcittFalseMismatch {
            stored: 0,
            calculated: 0,
        };
    }
    let split = bytes.len() - 2;
    let stored = u16::from_be_bytes([bytes[split], bytes[split + 1]]);
    let calculated = crc16_ccitt_false(&bytes[..split]);
    if stored == calculated {
        ChecksumCandidate::Crc16CcittFalseMatch(stored)
    } else {
        ChecksumCandidate::Crc16CcittFalseMismatch { stored, calculated }
    }
}

pub fn crc16_ccitt_false(bytes: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Versioned binary container; each sector body remains byte-for-byte raw.
pub fn encode_backup(
    info: &OnboardInfo,
    mode: OperatingMode,
    current: [u8; 3],
    sectors: &[RawSector],
) -> Result<Vec<u8>, FeatureError> {
    let mut out = Vec::new();
    out.extend_from_slice(b"G102OBK1");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&info.raw);
    out.push(match mode {
        OperatingMode::Onboard => 1,
        OperatingMode::Host => 2,
        OperatingMode::Unknown(v) => v,
    });
    out.extend_from_slice(&current);
    let count = u16::try_from(sectors.len())
        .map_err(|_| malformed("too many sectors for backup container"))?;
    out.extend_from_slice(&count.to_be_bytes());
    for sector in sectors {
        let len = u32::try_from(sector.bytes.len())
            .map_err(|_| malformed("sector too large for backup container"))?;
        out.extend_from_slice(&sector.id.to_be_bytes());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&sector.bytes);
    }
    Ok(out)
}

/// Parse G102OBK1 without relaxing record ordering, lengths, or trailing bytes.
/// Sector bodies are left untouched, including unknown profile components.
pub fn decode_backup(bytes: &[u8]) -> Result<RawBackup, FeatureError> {
    const HEADER: usize = 27;
    if bytes.len() < HEADER || &bytes[..8] != b"G102OBK1" {
        return Err(malformed("invalid or truncated G102OBK1 backup header"));
    }
    if u16::from_be_bytes([bytes[8], bytes[9]]) != 1 {
        return Err(malformed("unsupported G102 backup container version"));
    }
    let mut raw = [0; 11];
    raw.copy_from_slice(&bytes[10..21]);
    let info = OnboardInfo {
        memory_model: raw[0],
        profile_format: raw[1],
        macro_format: raw[2],
        profile_count: raw[3],
        factory_profile_count: raw[4],
        button_count: raw[5],
        sector_count: raw[6],
        sector_size: u16::from_be_bytes([raw[7], raw[8]]),
        mechanical_layout: raw[9],
        various_info: raw[10],
        raw,
    };
    validate_descriptor(&info)?;
    let mode = OperatingMode::from_wire(bytes[21]);
    let current_profile_raw = [bytes[22], bytes[23], bytes[24]];
    let count = u16::from_be_bytes([bytes[25], bytes[26]]) as usize;
    if count != info.sector_count as usize {
        return Err(malformed(
            "backup sector record count does not match descriptor",
        ));
    }
    let mut cursor = HEADER;
    let mut sectors = Vec::with_capacity(count);
    for expected_id in 0..count {
        let record_end = cursor
            .checked_add(6)
            .ok_or_else(|| malformed("backup offset overflow"))?;
        let header = bytes
            .get(cursor..record_end)
            .ok_or_else(|| malformed("truncated backup sector record header"))?;
        let id = u16::from_be_bytes([header[0], header[1]]);
        let len = u32::from_be_bytes([header[2], header[3], header[4], header[5]]) as usize;
        if id as usize != expected_id || len != info.sector_size as usize {
            return Err(malformed(
                "backup sector IDs or lengths do not match descriptor",
            ));
        }
        cursor = record_end;
        let end = cursor
            .checked_add(len)
            .ok_or_else(|| malformed("backup sector offset overflow"))?;
        let body = bytes
            .get(cursor..end)
            .ok_or_else(|| malformed("truncated backup sector data"))?;
        let data = body.to_vec();
        sectors.push(RawSector {
            id,
            checksum_candidate: checksum_candidate(&data),
            bytes: data,
        });
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err(malformed("backup contains trailing bytes"));
    }
    Ok(RawBackup {
        info,
        mode,
        current_profile_raw,
        sectors,
    })
}

/// Decode only the format-4 schema supported by the G102 backup evidence.
/// The base-v1 field locations are reverse-engineered reference schema, while
/// the matching fields/checksum below are corroborated by this specific dump.
pub fn decode_g102_format4_profile(
    info: &OnboardInfo,
    sector: &RawSector,
) -> Result<DecodedG102Profile, FeatureError> {
    if (
        info.memory_model,
        info.profile_format,
        info.macro_format,
        info.button_count,
        info.sector_size,
    ) != (1, 4, 1, 6, 255)
    {
        return Err(malformed(
            "G102 decoder requires memory model 0x01, profile format 0x04, macro format 0x01, six buttons, and 255-byte sectors",
        ));
    }
    let b = sector.bytes.as_slice();
    if b.len() != 255 || sector.id == 0 {
        return Err(malformed(
            "G102 format-4 profile sector must be a 255-byte non-directory sector",
        ));
    }
    let checksum = match checksum_candidate(b) {
        ChecksumCandidate::Crc16CcittFalseMatch(value) => value,
        ChecksumCandidate::Erased => return Err(malformed("profile sector is erased")),
        ChecksumCandidate::Crc16CcittFalseMismatch { .. } => {
            return Err(malformed(
                "profile sector CRC-16/CCITT-FALSE validation failed",
            ));
        }
    };
    // Recovered base-v1 offsets: rate 0, DPI block 1..0x0b, color 0x0d,
    // power 0x10, angle snap 0x11, four-byte actions beginning at 0x20.
    let dpi_stages = std::array::from_fn(|i| u16::from_le_bytes([b[3 + i * 2], b[4 + i * 2]]));
    let mut buttons = Vec::with_capacity(6);
    for slot in 0..6 {
        let at = 0x20 + slot * 4;
        let raw = [b[at], b[at + 1], b[at + 2], b[at + 3]];
        let action = ButtonAction::from_bytes(raw);
        buttons.push(DecodedButton {
            slot: slot + 1,
            raw,
            action,
        });
    }
    Ok(DecodedG102Profile {
        sector_id: sector.id,
        report_rate_raw: b[0],
        default_dpi_stage: b[1],
        shifted_dpi_stage: b[2],
        dpi_stages,
        color_raw: [b[0x0d], b[0x0e], b[0x0f]],
        power_raw: b[0x10],
        angle_snap_raw: b[0x11],
        buttons,
        checksum,
    })
}

pub fn create_button_remap_plan(
    backup: &RawBackup,
    remaps: &[(usize, VerifiedAction)],
) -> Result<ProfileEditPlan, FeatureError> {
    let info = &backup.info;
    if (
        info.memory_model,
        info.profile_format,
        info.macro_format,
        info.button_count,
        info.sector_size,
    ) != (1, 4, 1, 6, 255)
    {
        return Err(malformed(
            "G102 profile editor requires memory model 0x01, profile format 0x04, macro format 0x01, 6 buttons, and 255-byte sectors",
        ));
    }
    if remaps.is_empty() {
        return Err(malformed("no button remappings requested"));
    }
    for &(slot, _) in remaps {
        if slot < 1 || slot > info.button_count as usize {
            return Err(malformed("button slot out of range (must be 1..=6)"));
        }
    }
    let dir_sector = backup
        .sectors
        .iter()
        .find(|s| s.id == 0)
        .ok_or_else(|| malformed("backup lacks directory sector 0"))?;
    let active_slot = backup.current_profile_raw[1];
    if active_slot == 0 {
        return Err(malformed("backup reports no active profile slot"));
    }
    let entry_start = (active_slot as usize - 1) * 4;
    let entry = dir_sector
        .bytes
        .get(entry_start..entry_start + 4)
        .ok_or_else(|| malformed("active profile slot is outside directory"))?;
    let profile_sector_id = u16::from_be_bytes([entry[0], entry[1]]);
    if profile_sector_id == 0 || profile_sector_id as usize >= backup.sectors.len() {
        return Err(malformed(
            "active profile directory entry points to invalid sector",
        ));
    }

    let profile_sector = backup
        .sectors
        .iter()
        .find(|s| s.id == profile_sector_id)
        .ok_or_else(|| malformed("active profile sector missing from backup"))?;

    let old_checksum = match profile_sector.checksum_candidate {
        ChecksumCandidate::Crc16CcittFalseMatch(sum) => sum,
        ChecksumCandidate::Erased => {
            return Err(malformed("profile sector is erased; cannot edit"));
        }
        ChecksumCandidate::Crc16CcittFalseMismatch { .. } => {
            return Err(malformed(
                "original profile sector failed CRC-16 validation; refusing to edit corrupt profile",
            ));
        }
    };

    let mut new_bytes = profile_sector.bytes.clone();
    let mut changes = Vec::new();
    for &(slot, action) in remaps {
        let offset = 0x20 + (slot - 1) * 4;
        let old_raw = [
            new_bytes[offset],
            new_bytes[offset + 1],
            new_bytes[offset + 2],
            new_bytes[offset + 3],
        ];
        let new_raw = action.to_bytes();
        new_bytes[offset..offset + 4].copy_from_slice(&new_raw);
        changes.push(ButtonChange {
            slot,
            offset,
            old_raw,
            new_raw,
            old_action: ButtonAction::from_bytes(old_raw),
            new_action: action.to_button_action(),
        });
    }

    let new_checksum = crc16_ccitt_false(&new_bytes[..253]);
    new_bytes[253..255].copy_from_slice(&new_checksum.to_be_bytes());

    let mut byte_diffs = Vec::new();
    for (offset, (&old, &new)) in profile_sector
        .bytes
        .iter()
        .zip(new_bytes.iter())
        .enumerate()
    {
        if old != new {
            byte_diffs.push(ByteDiff {
                offset,
                old_byte: old,
                new_byte: new,
            });
        }
    }

    // Safety verification: verify that ONLY requested slots and CRC bytes were altered
    for diff in &byte_diffs {
        let in_button_table =
            diff.offset >= 0x20 && diff.offset < 0x20 + (info.button_count as usize) * 4;
        let in_crc = diff.offset >= 253 && diff.offset < 255;
        if !in_button_table && !in_crc {
            return Err(malformed(
                "integrity check failed: unintended byte modified outside button table and CRC",
            ));
        }
    }

    let mut modified_sectors = backup.sectors.clone();
    for sector in &mut modified_sectors {
        if sector.id == profile_sector_id {
            sector.bytes = new_bytes.clone();
            sector.checksum_candidate = ChecksumCandidate::Crc16CcittFalseMatch(new_checksum);
        }
    }

    let modified_backup = RawBackup {
        info: backup.info.clone(),
        mode: backup.mode,
        current_profile_raw: backup.current_profile_raw,
        sectors: modified_sectors,
    };

    Ok(ProfileEditPlan {
        profile_sector_id,
        changes,
        old_checksum,
        new_checksum,
        byte_diffs,
        modified_backup,
    })
}

pub fn compare_backups(a: &RawBackup, b: &RawBackup) -> BackupDiff {
    let mut sector_diffs = Vec::new();
    let metadata_changed =
        a.info != b.info || a.mode != b.mode || a.current_profile_raw != b.current_profile_raw;
    for (sec_a, sec_b) in a.sectors.iter().zip(b.sectors.iter()) {
        let mut diffs = Vec::new();
        for (offset, (&ba, &bb)) in sec_a.bytes.iter().zip(sec_b.bytes.iter()).enumerate() {
            if ba != bb {
                diffs.push(ByteDiff {
                    offset,
                    old_byte: ba,
                    new_byte: bb,
                });
            }
        }
        if !diffs.is_empty() {
            sector_diffs.push((sec_a.id, diffs));
        }
    }
    BackupDiff {
        sector_diffs,
        metadata_changed,
    }
}

pub fn save_backup_file(
    output_path: &str,
    backup: &RawBackup,
    input_path: Option<&str>,
) -> Result<(), FeatureError> {
    if let Some(in_path) = input_path
        && std::path::Path::new(output_path) == std::path::Path::new(in_path)
    {
        return Err(malformed(
            "refusing to overwrite original backup file; specify a different output path",
        ));
    }
    let bytes = encode_backup(
        &backup.info,
        backup.mode,
        backup.current_profile_raw,
        &backup.sectors,
    )?;
    std::fs::write(output_path, bytes).map_err(|e| {
        FeatureError::Client(crate::error::ClientError::Transport(format!(
            "could not write backup file `{output_path}`: {e}"
        )))
    })
}

pub fn read_writable_memory<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
    info: &OnboardInfo,
) -> Result<Vec<RawSector>, FeatureError> {
    validate_descriptor(info)?;
    let mut sectors = Vec::with_capacity(info.sector_count as usize);
    for sector in 0..info.sector_count as u16 {
        sectors.push(read_sector(
            client,
            index,
            sector,
            info.sector_size as usize,
        )?);
    }
    Ok(sectors)
}

/// Configurable policy default for `FN_END_WRITE` acknowledgement waiting.
///
/// Two seconds is a conservative software default, not a measured G102 latency
/// or protocol guarantee. The target's worst-case commit latency is unknown.
/// Never automatically retry when this deadline expires.
pub const DEFAULT_COMMIT_TIMEOUT: Duration = Duration::from_millis(2000);

/// No default is provided for the byte needed to fill a final partial 16-byte
/// transfer. The G102's supplied backup establishes 255-byte sectors, but does
/// not establish whether the transfer's 256th byte is ignored, consumed, or
/// required to have a particular value. A caller must explicitly supply a
/// value; doing so is not evidence that the value is G102-compatible.
///
/// Typed state of a write transaction at the moment an error is detected or a
/// report is returned. Each variant maps to a distinct recovery posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTransactionState {
    /// No `FN_END_WRITE` request was sent. The host therefore has no requested
    /// commit, but G102 staging/flash behavior and cleanup on disconnect or
    /// power cycle are not hardware-validated.
    Uncommitted,

    /// The `FN_END_WRITE` request was attempted but no trustworthy completion
    /// result was obtained (timeout, disconnect, malformed response, or device
    /// error). The MCU may have committed, may still be programming, or the
    /// response may have been lost. The exact error semantics are unverified.
    ///
    /// **A retry is strictly forbidden.** Issuing another `FN_START_WRITE` /
    /// `FN_WRITE_DATA` / `FN_END_WRITE` sequence before the first commit has
    /// settled could cause double-write flash wear, a partial second write over
    /// a partially-committed sector, or MCU-firmware undefined behaviour.
    PotentiallyCommitted,

    /// `FN_END_WRITE` was acknowledged successfully, but readback verification
    /// has not yet succeeded. The acknowledgement alone does not establish the
    /// resulting sector contents.
    Committed,

    /// The intended full sector was read back and passed byte-for-byte and
    /// format-specific verification.
    Verified,
}

impl fmt::Display for WriteTransactionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncommitted => f.write_str("UNCOMMITTED"),
            Self::PotentiallyCommitted => f.write_str("POTENTIALLY COMMITTED"),
            Self::Committed => f.write_str("COMMITTED"),
            Self::Verified => f.write_str("VERIFIED"),
        }
    }
}

/// Per-operation write configuration.
///
/// The defaults use a 2 000 ms commit timeout and no final-chunk byte. A
/// non-aligned transfer therefore fails before `FN_START_WRITE` unless the
/// caller explicitly supplies the unknown tail byte. See [`DEFAULT_COMMIT_TIMEOUT`]
/// for the unresolved hardware-dependent timeout uncertainty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// Timeout for the `FN_END_WRITE` acknowledgement.
    ///
    /// Flash commit on embedded MCUs can be significantly slower than normal
    /// HID++ round-trips. This timeout is applied **only** to `FN_END_WRITE`
    /// and does not affect data-chunk or start-write timeouts. A timeout on
    /// `FN_END_WRITE` produces a [`WriteTransactionState::PotentiallyCommitted`]
    /// error and must never be followed by an automatic retry.
    pub commit_timeout: Duration,

    /// Explicit byte for a final partial 16-byte chunk. `None` fails closed
    /// before the write transaction starts if `data.len()` is not 16-byte
    /// aligned. Any supplied byte is an unverified caller choice, not a
    /// validated G102 protocol value.
    pub tail_padding_byte: Option<u8>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            commit_timeout: DEFAULT_COMMIT_TIMEOUT,
            tail_padding_byte: None,
        }
    }
}

#[derive(Debug)]
pub enum WriteProgressError {
    PreflightRefused(FeatureError),
    StartWriteFailed(FeatureError),
    WriteChunkFailed {
        offset: usize,
        error: FeatureError,
    },
    /// The `FN_END_WRITE` request was attempted but its outcome was not
    /// established. No retry or second write may be attempted.
    CommitFailed {
        error: FeatureError,
        /// Typed flash-state at the point of failure.  Callers must inspect this
        /// before deciding on any recovery action.
        state: WriteTransactionState,
        /// Whether the reported failure was a timeout. All errors after the
        /// commit request are potentially committed, not only timeouts.
        ambiguous_timeout: bool,
        timeout: Duration,
        backup_path: std::path::PathBuf,
    },
    ReadBackFailed {
        error: FeatureError,
        backup_path: std::path::PathBuf,
    },
    ReadBackMismatch {
        offset: usize,
        expected: u8,
        actual: u8,
        backup_path: std::path::PathBuf,
    },
    ChecksumMismatch {
        stored: u16,
        calculated: u16,
        backup_path: std::path::PathBuf,
    },
    DecodedProfileMismatch {
        reason: String,
        backup_path: std::path::PathBuf,
    },
}

impl fmt::Display for WriteProgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreflightRefused(e) => write!(
                f,
                "write preflight refused: {e}.\n\
                 {}: No write request was sent and flash was not touched.",
                WriteTransactionState::Uncommitted
            ),
            Self::StartWriteFailed(e) => write!(
                f,
                "failed to initiate write window (FN_START_WRITE): {e}.\n\
                 {}: No commit request was sent; device-side staging behavior is unverified.",
                WriteTransactionState::Uncommitted
            ),
            Self::WriteChunkFailed { offset, error } => write!(
                f,
                "sector transfer failed at byte offset {offset} (FN_WRITE_DATA): {error}.\n\
                 {}: No commit request was sent. Partial device-side staging and whether disconnect/power-cycle clears it are unverified; do not infer cleanup behavior.",
                WriteTransactionState::Uncommitted
            ),
            Self::CommitFailed {
                error,
                state,
                ambiguous_timeout,
                timeout,
                backup_path,
            } => {
                write!(
                    f,
                    "FN_END_WRITE outcome is unknown: {error}. {} Configured acknowledgement timeout was {timeout:?}.\n\
                     {state}: The request may have reached the device; it may still be programming, may have committed without an acknowledgement, or the response/transport may have failed.\n\
                     CRITICAL: Do not retry, restore, or issue any other write while commit status is unknown.\n\
                     READ-ONLY RECOVERY: Allow the device to settle, reconnect if needed, then export current memory and compare it with the preserved pre-write backup:\n\
                       g102-control onboard export after-commit.obk\n\
                       g102-control onboard diff {} after-commit.obk\n\
                     If the fresh export is unavailable or the difference is not fully understood, stop and preserve both files for manual recovery. Backup path: `{}`. Failure was a timeout: {}.",
                    if *ambiguous_timeout {
                        "No matching acknowledgement arrived before the deadline."
                    } else {
                        "A transport or device error occurred after the commit request was attempted."
                    },
                    backup_path.display(),
                    backup_path.display(),
                    ambiguous_timeout
                )
            }
            Self::ReadBackFailed { error, backup_path } => write!(
                f,
                "sector was committed, but readback failed: {error}.\n\
                 {}: The commit was acknowledged, but contents are not verified. Run `g102-control onboard export after-write.obk`, then `g102-control onboard diff {} after-write.obk`. Do not write until the current sector is understood. Preserve the pre-write backup at `{}`.",
                WriteTransactionState::Committed,
                backup_path.display(),
                backup_path.display()
            ),
            Self::ReadBackMismatch {
                offset,
                expected,
                actual,
                backup_path,
            } => write!(
                f,
                "readback verification mismatch at byte offset {offset} (0x{offset:02x}): expected 0x{expected:02x}, read back 0x{actual:02x}.\n\
                 {}: Intended data does not match flash contents.\n\
                 Run `g102-control onboard export after-write.obk`, then `g102-control onboard diff {} after-write.obk`. Do not issue another write until current memory is understood.",
                WriteTransactionState::Committed,
                backup_path.display()
            ),
            Self::ChecksumMismatch {
                stored,
                calculated,
                backup_path,
            } => write!(
                f,
                "readback sector checksum mismatch: stored CRC 0x{stored:04x}, calculated CRC 0x{calculated:04x}.\n\
                 {} (INTEGRITY COMPROMISED): Sector CRC validation failed.\n\
                 Run `g102-control onboard export after-write.obk`, then `g102-control onboard diff {} after-write.obk`. Do not issue another write until current memory is understood.",
                WriteTransactionState::Committed,
                backup_path.display()
            ),
            Self::DecodedProfileMismatch {
                reason,
                backup_path,
            } => write!(
                f,
                "readback sector profile decode failed: {reason}.\n\
                 {} (PROFILE DECODE FAILURE): Sector buttons do not match expected assignments.\n\
                 Run `g102-control onboard export after-write.obk`, then `g102-control onboard diff {} after-write.obk`. Do not issue another write until current memory is understood.",
                WriteTransactionState::Committed,
                backup_path.display()
            ),
        }
    }
}

impl std::error::Error for WriteProgressError {}

impl WriteProgressError {
    /// State of the device write transaction represented by this failure.
    pub fn transaction_state(&self) -> WriteTransactionState {
        match self {
            Self::PreflightRefused(_)
            | Self::StartWriteFailed(_)
            | Self::WriteChunkFailed { .. } => WriteTransactionState::Uncommitted,
            Self::CommitFailed { state, .. } => *state,
            Self::ReadBackFailed { .. }
            | Self::ReadBackMismatch { .. }
            | Self::ChecksumMismatch { .. }
            | Self::DecodedProfileMismatch { .. } => WriteTransactionState::Committed,
        }
    }
}

#[derive(Debug)]
pub enum WriteError {
    SafetyRefusal(String),
    Feature(FeatureError),
    Io(std::io::Error),
    Progress(WriteProgressError),
    InvalidBackup(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SafetyRefusal(msg) => write!(f, "safety refusal: {msg}"),
            Self::Feature(e) => write!(f, "feature error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Progress(e) => write!(f, "write transaction error: {e}"),
            Self::InvalidBackup(msg) => write!(f, "invalid backup: {msg}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<FeatureError> for WriteError {
    fn from(e: FeatureError) -> Self {
        Self::Feature(e)
    }
}

impl From<std::io::Error> for WriteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<WriteProgressError> for WriteError {
    fn from(e: WriteProgressError) -> Self {
        Self::Progress(e)
    }
}

/// Physical G102 profile writes remain disabled until model/firmware-specific
/// write behavior, including the final transfer byte and commit semantics, is
/// hardware-validated and documented.
pub const PROFILE_WRITE_GATE_MESSAGE: &str = "physical onboard profile writes are disabled pending exact G102 model/firmware validation; --confirm-write cannot override this gate. Read-only export and dry-run remain available";

fn profile_write_validation_available() -> bool {
    // Deliberately hard-disabled. Change only after the hardware evidence gate
    // in VALIDATION.md has been met and reviewed.
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteExecutionReport {
    pub state: WriteTransactionState,
    pub backup_path: std::path::PathBuf,
    pub sector_written: u16,
    pub bytes_written: usize,
    pub changes: Vec<ButtonChange>,
    pub old_checksum: u16,
    pub new_checksum: u16,
    pub readback_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreExecutionReport {
    pub state: WriteTransactionState,
    pub safety_backup_path: std::path::PathBuf,
    pub restore_file: std::path::PathBuf,
    pub sectors_written: Vec<u16>,
    pub readback_verified: bool,
}

pub fn generate_unique_backup_path(prefix: &str) -> std::path::PathBuf {
    generate_unique_backup_path_with_exists(prefix, |candidate| candidate.exists())
}

fn generate_unique_backup_path_with_exists(
    prefix: &str,
    mut exists: impl FnMut(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut candidate = std::path::PathBuf::from(format!("{prefix}_{secs}.obk"));
    let mut counter = 1;
    while exists(&candidate) {
        candidate = std::path::PathBuf::from(format!("{prefix}_{secs}_{counter}.obk"));
        counter += 1;
    }
    candidate
}

fn write_sector_with_tracking<T: HidTransport>(
    client: &mut HidppClient<T>,
    index: u8,
    sector: u16,
    data: &[u8],
    options: Option<&WriteOptions>,
    backup_path: Option<&std::path::Path>,
) -> Result<(), WriteProgressError> {
    let default_options = WriteOptions::default();
    let opts = options.unwrap_or(&default_options);

    if data.is_empty() || data.len() > MAX_SECTOR_SIZE {
        return Err(WriteProgressError::PreflightRefused(malformed(
            "sector data is empty or exceeds MAX_SECTOR_SIZE",
        )));
    }
    if opts.commit_timeout.is_zero() {
        return Err(WriteProgressError::PreflightRefused(malformed(
            "FN_END_WRITE acknowledgement timeout must be greater than zero",
        )));
    }
    if !data.len().is_multiple_of(READ_SIZE) && opts.tail_padding_byte.is_none() {
        return Err(WriteProgressError::PreflightRefused(malformed(
            "non-aligned sector transfer requires an explicit final-chunk byte; G102-compatible handling is unverified",
        )));
    }
    let length = u16::try_from(data.len()).map_err(|_| {
        WriteProgressError::PreflightRefused(malformed("sector length exceeds u16::MAX"))
    })?;
    let [sh, sl] = sector.to_be_bytes();
    let [lh, ll] = length.to_be_bytes();
    let start_params = [sh, sl, 0, 0, lh, ll];
    client
        .request(index, FN_START_WRITE, &start_params)
        .map_err(|e| WriteProgressError::StartWriteFailed(FeatureError::Client(e)))?;

    for (chunk_idx, chunk) in data.chunks(READ_SIZE).enumerate() {
        let mut chunk_buf = [0; READ_SIZE];
        if chunk.len() < READ_SIZE {
            // The preflight above guarantees this option exists. The value is
            // explicit caller policy, not a G102-validated protocol constant.
            chunk_buf.fill(
                opts.tail_padding_byte
                    .expect("partial chunk was preflighted"),
            );
        }
        chunk_buf[..chunk.len()].copy_from_slice(chunk);
        if let Err(e) = client.request(index, FN_WRITE_DATA, &chunk_buf) {
            return Err(WriteProgressError::WriteChunkFailed {
                offset: chunk_idx * READ_SIZE,
                error: FeatureError::Client(e),
            });
        }
    }

    let commit_res = client.request_with_timeout(index, FN_END_WRITE, &[], opts.commit_timeout);
    if let Err(e) = commit_res {
        let is_timeout = matches!(&e, ClientError::Protocol(ProtocolError::Timeout));
        return Err(WriteProgressError::CommitFailed {
            error: FeatureError::Client(e),
            // Once the commit request path has been entered, timeout, transport
            // loss, malformed replies, and device errors are all ambiguous
            // until the exact firmware's commit semantics are established.
            state: WriteTransactionState::PotentiallyCommitted,
            ambiguous_timeout: is_timeout,
            timeout: opts.commit_timeout,
            backup_path: backup_path
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("<unknown>")),
        });
    }

    Ok(())
}

pub fn execute_profile_button_remap<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    remaps: &[(usize, VerifiedAction)],
    confirm_write: bool,
    custom_backup_path: Option<&std::path::Path>,
    options: Option<WriteOptions>,
) -> Result<WriteExecutionReport, WriteError> {
    if !profile_write_validation_available() {
        return Err(WriteError::SafetyRefusal(
            PROFILE_WRITE_GATE_MESSAGE.to_owned(),
        ));
    }
    execute_profile_button_remap_after_gate(
        client,
        feature_index,
        remaps,
        confirm_write,
        custom_backup_path,
        options,
    )
}

fn execute_profile_button_remap_after_gate<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    remaps: &[(usize, VerifiedAction)],
    confirm_write: bool,
    custom_backup_path: Option<&std::path::Path>,
    options: Option<WriteOptions>,
) -> Result<WriteExecutionReport, WriteError> {
    if !confirm_write {
        return Err(WriteError::SafetyRefusal(
            "Writing to physical mouse memory requires explicit confirmation flag --confirm-write. Use --dry-run to preview changes safely.".into(),
        ));
    }
    if remaps.is_empty() {
        return Err(WriteError::SafetyRefusal(
            "No button remaps specified".into(),
        ));
    }

    let info = read_info(client, feature_index)?;
    validate_descriptor(&info)?;
    let mode = read_mode(client, feature_index)?;
    let current_profile_raw = read_current_profile_raw(client, feature_index)?;
    let sectors = read_writable_memory(client, feature_index, &info)?;
    let live_backup = RawBackup {
        info: info.clone(),
        mode,
        current_profile_raw,
        sectors,
    };

    if live_backup.sectors.len() < 2 {
        return Err(WriteError::SafetyRefusal(
            "Device does not have profile sector 1".into(),
        ));
    }
    let _decoded_before = decode_g102_format4_profile(&info, &live_backup.sectors[1])?;

    // Pre-write safety backup
    let backup_path = match custom_backup_path {
        Some(path) => {
            if path.exists() {
                return Err(WriteError::SafetyRefusal(format!(
                    "specified backup file `{}` already exists; refusing to overwrite",
                    path.display()
                )));
            }
            path.to_path_buf()
        }
        None => generate_unique_backup_path("g102_backup"),
    };
    let path_str = backup_path
        .to_str()
        .ok_or_else(|| malformed("backup path is not valid utf-8"))?;
    save_backup_file(path_str, &live_backup, None)?;

    // Verify written backup on disk before any flash modification
    let saved_bytes = std::fs::read(&backup_path)?;
    let disk_backup = decode_backup(&saved_bytes).map_err(|e| {
        WriteError::InvalidBackup(format!("saved backup could not be verified on disk: {e}"))
    })?;
    if disk_backup != live_backup {
        return Err(WriteError::InvalidBackup(
            "saved backup on disk does not match live mouse state".into(),
        ));
    }

    // Build the remap plan and verify integrity
    let plan = create_button_remap_plan(&live_backup, remaps)?;
    if plan.profile_sector_id != 1 {
        return Err(WriteError::SafetyRefusal(
            "remap plan targeted invalid sector (expected sector 1)".into(),
        ));
    }
    let target_sector_bytes = &plan.modified_backup.sectors[1].bytes;

    // Write sector 1 to device
    write_sector_with_tracking(
        client,
        feature_index,
        1,
        target_sector_bytes,
        options.as_ref(),
        Some(&backup_path),
    )?;

    // Read back sector 1
    let readback =
        read_sector(client, feature_index, 1, target_sector_bytes.len()).map_err(|e| {
            WriteProgressError::ReadBackFailed {
                error: e,
                backup_path: backup_path.clone(),
            }
        })?;

    // Verify readback byte-for-byte
    if readback.bytes.len() != target_sector_bytes.len() {
        return Err(WriteProgressError::ReadBackMismatch {
            offset: 0,
            expected: target_sector_bytes.len() as u8,
            actual: readback.bytes.len() as u8,
            backup_path: backup_path.clone(),
        }
        .into());
    }
    for (offset, (&expected, &actual)) in target_sector_bytes
        .iter()
        .zip(readback.bytes.iter())
        .enumerate()
    {
        if expected != actual {
            return Err(WriteProgressError::ReadBackMismatch {
                offset,
                expected,
                actual,
                backup_path: backup_path.clone(),
            }
            .into());
        }
    }

    // Verify readback checksum
    let expected_crc = plan.new_checksum;
    match readback.checksum_candidate {
        ChecksumCandidate::Crc16CcittFalseMatch(crc) if crc == expected_crc => {}
        ChecksumCandidate::Crc16CcittFalseMismatch { stored, calculated } => {
            return Err(WriteProgressError::ChecksumMismatch {
                stored,
                calculated,
                backup_path: backup_path.clone(),
            }
            .into());
        }
        _ => {
            return Err(WriteProgressError::ChecksumMismatch {
                stored: 0,
                calculated: expected_crc,
                backup_path: backup_path.clone(),
            }
            .into());
        }
    }

    // Verify decoded profile assignments
    let decoded_after = decode_g102_format4_profile(&info, &readback).map_err(|e| {
        WriteProgressError::DecodedProfileMismatch {
            reason: e.to_string(),
            backup_path: backup_path.clone(),
        }
    })?;
    for change in &plan.changes {
        let button = decoded_after
            .buttons
            .iter()
            .find(|b| b.slot == change.slot)
            .ok_or_else(|| WriteProgressError::DecodedProfileMismatch {
                reason: format!("slot {} missing from decoded readback", change.slot),
                backup_path: backup_path.clone(),
            })?;
        if button.action != change.new_action {
            return Err(WriteProgressError::DecodedProfileMismatch {
                reason: format!(
                    "slot {} readback action mismatch: got {:?}, expected {:?}",
                    change.slot, button.action, change.new_action
                ),
                backup_path: backup_path.clone(),
            }
            .into());
        }
    }

    Ok(WriteExecutionReport {
        state: WriteTransactionState::Verified,
        backup_path,
        sector_written: 1,
        bytes_written: target_sector_bytes.len(),
        changes: plan.changes,
        old_checksum: plan.old_checksum,
        new_checksum: plan.new_checksum,
        readback_verified: true,
    })
}

pub fn execute_profile_restore<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    restore_backup_path: &std::path::Path,
    confirm_write: bool,
    custom_backup_path: Option<&std::path::Path>,
    options: Option<WriteOptions>,
) -> Result<RestoreExecutionReport, WriteError> {
    if !profile_write_validation_available() {
        return Err(WriteError::SafetyRefusal(
            PROFILE_WRITE_GATE_MESSAGE.to_owned(),
        ));
    }
    execute_profile_restore_after_gate(
        client,
        feature_index,
        restore_backup_path,
        confirm_write,
        custom_backup_path,
        options,
    )
}

fn execute_profile_restore_after_gate<T: HidTransport>(
    client: &mut HidppClient<T>,
    feature_index: u8,
    restore_backup_path: &std::path::Path,
    confirm_write: bool,
    custom_backup_path: Option<&std::path::Path>,
    options: Option<WriteOptions>,
) -> Result<RestoreExecutionReport, WriteError> {
    if !confirm_write {
        return Err(WriteError::SafetyRefusal(
            "Restoring backup to mouse flash requires explicit confirmation flag --confirm-write. Use --dry-run to preview restore changes.".into(),
        ));
    }

    let restore_bytes = std::fs::read(restore_backup_path)?;
    let restore_backup = decode_backup(&restore_bytes)
        .map_err(|e| WriteError::InvalidBackup(format!("failed to parse restore backup: {e}")))?;
    validate_descriptor(&restore_backup.info)?;
    if restore_backup.sectors.len() < 2 {
        return Err(WriteError::InvalidBackup(
            "restore backup has fewer than 2 sectors".into(),
        ));
    }
    let _ = decode_g102_format4_profile(&restore_backup.info, &restore_backup.sectors[1])?;

    let live_info = read_info(client, feature_index)?;
    validate_descriptor(&live_info)?;
    if live_info != restore_backup.info {
        return Err(WriteError::SafetyRefusal(
            "connected mouse descriptor does not match restore backup descriptor".into(),
        ));
    }
    let mode = read_mode(client, feature_index)?;
    let current_profile_raw = read_current_profile_raw(client, feature_index)?;
    let live_sectors = read_writable_memory(client, feature_index, &live_info)?;
    let live_backup = RawBackup {
        info: live_info,
        mode,
        current_profile_raw,
        sectors: live_sectors,
    };

    let diff = compare_backups(&live_backup, &restore_backup);
    if diff.sector_diffs.is_empty() {
        return Err(WriteError::SafetyRefusal(
            "connected mouse memory already matches restore backup; no sectors need writing".into(),
        ));
    }
    for (sector_id, _) in &diff.sector_diffs {
        if *sector_id != 1 {
            return Err(WriteError::SafetyRefusal(format!(
                "restore backup differs in sector {sector_id}; safe restore currently only permits modifying profile sector 1"
            )));
        }
    }

    let safety_backup_path = match custom_backup_path {
        Some(path) => {
            if path.exists() {
                return Err(WriteError::SafetyRefusal(format!(
                    "safety backup path `{}` already exists; refusing to overwrite",
                    path.display()
                )));
            }
            path.to_path_buf()
        }
        None => generate_unique_backup_path("g102_prerestore_backup"),
    };
    let safety_path_str = safety_backup_path
        .to_str()
        .ok_or_else(|| malformed("safety backup path is not valid utf-8"))?;
    save_backup_file(safety_path_str, &live_backup, restore_backup_path.to_str())?;

    // Verify safety backup on disk
    let saved_bytes = std::fs::read(&safety_backup_path)?;
    let verified_safety = decode_backup(&saved_bytes).map_err(|e| {
        WriteError::InvalidBackup(format!(
            "saved safety backup could not be verified on disk: {e}"
        ))
    })?;
    if verified_safety != live_backup {
        return Err(WriteError::InvalidBackup(
            "saved safety backup on disk does not match live mouse state".into(),
        ));
    }

    let target_sector_bytes = &restore_backup.sectors[1].bytes;
    write_sector_with_tracking(
        client,
        feature_index,
        1,
        target_sector_bytes,
        options.as_ref(),
        Some(&safety_backup_path),
    )?;

    let readback =
        read_sector(client, feature_index, 1, target_sector_bytes.len()).map_err(|e| {
            WriteProgressError::ReadBackFailed {
                error: e,
                backup_path: safety_backup_path.clone(),
            }
        })?;

    if readback.bytes.len() != target_sector_bytes.len() {
        return Err(WriteProgressError::ReadBackMismatch {
            offset: 0,
            expected: target_sector_bytes.len() as u8,
            actual: readback.bytes.len() as u8,
            backup_path: safety_backup_path.clone(),
        }
        .into());
    }
    for (offset, (&expected, &actual)) in target_sector_bytes
        .iter()
        .zip(readback.bytes.iter())
        .enumerate()
    {
        if expected != actual {
            return Err(WriteProgressError::ReadBackMismatch {
                offset,
                expected,
                actual,
                backup_path: safety_backup_path.clone(),
            }
            .into());
        }
    }

    match (
        readback.checksum_candidate,
        restore_backup.sectors[1].checksum_candidate,
    ) {
        (
            ChecksumCandidate::Crc16CcittFalseMatch(a),
            ChecksumCandidate::Crc16CcittFalseMatch(b),
        ) if a == b => {}
        (ChecksumCandidate::Crc16CcittFalseMismatch { stored, calculated }, _) => {
            return Err(WriteProgressError::ChecksumMismatch {
                stored,
                calculated,
                backup_path: safety_backup_path.clone(),
            }
            .into());
        }
        _ => {
            return Err(WriteProgressError::ChecksumMismatch {
                stored: 0,
                calculated: 0,
                backup_path: safety_backup_path.clone(),
            }
            .into());
        }
    }

    let _ = decode_g102_format4_profile(&restore_backup.info, &readback).map_err(|e| {
        WriteProgressError::DecodedProfileMismatch {
            reason: e.to_string(),
            backup_path: safety_backup_path.clone(),
        }
    })?;

    Ok(RestoreExecutionReport {
        state: WriteTransactionState::Verified,
        safety_backup_path,
        restore_file: restore_backup_path.to_path_buf(),
        sectors_written: vec![1],
        readback_verified: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Packet, ReportKind};
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    struct TestTempDir(std::path::PathBuf);

    impl TestTempDir {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "g102-control-onboard-{}-{}",
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("could not create test temporary directory: {error}"),
                }
            }
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
    fn reply(function: u8, sw: u8, params: &[u8]) -> Vec<u8> {
        Packet::new(ReportKind::Long, 0xff, 0x0f, function, sw, params)
            .unwrap()
            .encode()
    }
    fn mock() -> Mock {
        let mut directory = [0xff; 32];
        directory[..4].copy_from_slice(&[0, 1, 1, 0xff]);
        let profile = [0xff; 32];
        Mock {
            replies: VecDeque::from([
                reply(0, 0x0b, &[1, 0x63, 1, 1, 0, 5, 2, 0, 32, 0, 0]),
                reply(2, 0x0c, &[1]),
                reply(4, 0x0d, &[0, 1, 0]),
                reply(5, 0x0e, &directory[..16]),
                reply(5, 0x0f, &directory[16..]),
                reply(5, 1, &profile[..16]),
                reply(5, 2, &profile[16..]),
            ]),
            writes: vec![],
        }
    }

    // The simulator tests choose 0xff only as an explicit fixture value to
    // exercise packet sequencing. It is not asserted to be G102-compatible.
    const SIMULATED_TAIL_BYTE: u8 = 0xff;

    fn test_options(options: Option<WriteOptions>) -> WriteOptions {
        let mut options = options.unwrap_or_default();
        if options.tail_padding_byte.is_none() {
            options.tail_padding_byte = Some(SIMULATED_TAIL_BYTE);
        }
        options
    }

    fn write_sector_with_tracking<T: HidTransport>(
        client: &mut HidppClient<T>,
        index: u8,
        sector: u16,
        data: &[u8],
        options: Option<&WriteOptions>,
        backup_path: Option<&std::path::Path>,
    ) -> Result<(), WriteProgressError> {
        let options = test_options(options.copied());
        super::write_sector_with_tracking(client, index, sector, data, Some(&options), backup_path)
    }

    fn execute_profile_button_remap<T: HidTransport>(
        client: &mut HidppClient<T>,
        feature_index: u8,
        remaps: &[(usize, VerifiedAction)],
        confirm_write: bool,
        custom_backup_path: Option<&std::path::Path>,
        options: Option<WriteOptions>,
    ) -> Result<WriteExecutionReport, WriteError> {
        let options = test_options(options);
        super::execute_profile_button_remap_after_gate(
            client,
            feature_index,
            remaps,
            confirm_write,
            custom_backup_path,
            Some(options),
        )
    }

    fn execute_profile_restore<T: HidTransport>(
        client: &mut HidppClient<T>,
        feature_index: u8,
        restore_backup_path: &std::path::Path,
        confirm_write: bool,
        custom_backup_path: Option<&std::path::Path>,
        options: Option<WriteOptions>,
    ) -> Result<RestoreExecutionReport, WriteError> {
        let options = test_options(options);
        super::execute_profile_restore_after_gate(
            client,
            feature_index,
            restore_backup_path,
            confirm_write,
            custom_backup_path,
            Some(options),
        )
    }

    #[test]
    fn inspection_reads_descriptor_mode_directory_and_profile_without_mutation() {
        let mut client = HidppClient::new(mock(), 0xff, ReportKind::Long);
        let result = inspect(&mut client, 0x0f).unwrap();
        assert_eq!(result.info.profile_format, 0x63);
        assert_eq!(result.info.button_count, 5);
        assert_eq!(result.mode, OperatingMode::Onboard);
        assert_eq!(result.current_profile_raw, [0, 1, 0]);
        assert_eq!(result.current_profile, Some(1));
        assert_eq!(result.user_directory[0].sector, 1);
        assert_eq!(result.profiles[0].id, 1);
        assert_eq!(
            result.profiles[0].checksum_candidate,
            ChecksumCandidate::Erased
        );
        let transport = client.into_transport();
        assert_eq!(transport.writes.len(), 7);
        assert!(transport.writes.iter().all(|packet| packet[2] == 0x0f
            && packet[3] >> 4 != 1
            && packet[3] >> 4 != 3
            && packet[3] >> 4 != 6
            && packet[3] >> 4 != 7
            && packet[3] >> 4 != 8));
    }

    #[test]
    fn reads_unaligned_sector_tail_with_overlapping_aligned_chunk() {
        assert_eq!(read_offsets(255).last(), Some(&239));
        assert_eq!(read_offsets(256).last(), Some(&240));
        assert_eq!(read_offsets(15), Vec::<usize>::new());
    }

    #[test]
    fn crc_matches_standard_check_vector_and_erased_sector_is_not_checked() {
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29b1);
        let bytes = [0xff; 255];
        assert_eq!(checksum_candidate(&bytes), ChecksumCandidate::Erased);
    }

    #[test]
    fn backup_container_keeps_sector_bytes_and_descriptor() {
        let info = OnboardInfo {
            memory_model: 1,
            profile_format: 0x63,
            macro_format: 1,
            profile_count: 1,
            factory_profile_count: 0,
            button_count: 5,
            sector_count: 1,
            sector_size: 16,
            mechanical_layout: 0,
            various_info: 0,
            raw: [1, 0x63, 1, 1, 0, 5, 1, 0, 16, 0, 0],
        };
        let sector = RawSector {
            id: 0,
            bytes: vec![0x5a; 16],
            checksum_candidate: ChecksumCandidate::Crc16CcittFalseMismatch {
                stored: 0,
                calculated: 0,
            },
        };
        let backup = encode_backup(&info, OperatingMode::Host, [0, 0, 0], &[sector]).unwrap();
        assert_eq!(&backup[..8], b"G102OBK1");
        assert_eq!(u16::from_be_bytes([backup[8], backup[9]]), 1);
        assert_eq!(&backup[21..27], &[2, 0, 0, 0, 0, 1]);
        assert_eq!(&backup[33..], &[0x5a; 16]);
    }

    #[test]
    fn decodes_supplied_g102_backup_and_validates_both_programmed_sectors() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        assert_eq!(
            (
                backup.info.memory_model,
                backup.info.profile_format,
                backup.info.macro_format
            ),
            (1, 4, 1)
        );
        assert_eq!(
            (
                backup.info.button_count,
                backup.info.sector_count,
                backup.info.sector_size
            ),
            (6, 16, 255)
        );
        assert_eq!(backup.current_profile_raw, [0, 1, 0]);
        assert_eq!(backup.sectors.len(), 16);
        assert_eq!(backup.sectors[0].bytes.len(), 255);
        assert_eq!(backup.sectors[1].bytes[0], 1);
        assert_eq!(
            backup.sectors[0].checksum_candidate,
            ChecksumCandidate::Crc16CcittFalseMatch(0x587c)
        );
        assert_eq!(
            backup.sectors[1].checksum_candidate,
            ChecksumCandidate::Crc16CcittFalseMatch(0xb3d1)
        );
        assert!(
            backup.sectors[2..]
                .iter()
                .all(|s| s.checksum_candidate == ChecksumCandidate::Erased)
        );
        let decoded = decode_g102_format4_profile(&backup.info, &backup.sectors[1]).unwrap();
        assert_eq!(decoded.report_rate_raw, 1);
        assert_eq!(
            (decoded.default_dpi_stage, decoded.shifted_dpi_stage),
            (1, 0)
        );
        assert_eq!(decoded.dpi_stages, [1000, 1600, 2200, 3200, 4200]);
        assert_eq!(decoded.buttons.len(), 6);
        assert_eq!(decoded.buttons[0].raw, [0x80, 1, 0, 1]);
        assert!(matches!(
            decoded.buttons[0].action,
            ButtonAction::MouseButton { mask: 1, .. }
        ));
        assert!(matches!(
            decoded.buttons[5].action,
            ButtonAction::Special { code: 5, .. }
        ));
    }

    #[test]
    fn synthetic_format4_sector_decodes_fields_and_rejects_bad_checksum() {
        let info = OnboardInfo {
            memory_model: 1,
            profile_format: 4,
            macro_format: 1,
            profile_count: 1,
            factory_profile_count: 0,
            button_count: 6,
            sector_count: 16,
            sector_size: 255,
            mechanical_layout: 10,
            various_info: 1,
            raw: [1, 4, 1, 1, 0, 6, 16, 0, 255, 10, 1],
        };
        let mut bytes = [0xff; 255];
        bytes[0] = 1;
        bytes[1] = 2;
        bytes[2] = 3;
        bytes[3..13].copy_from_slice(&[0xe8, 3, 0x40, 6, 0x98, 8, 0x80, 0x0c, 0x68, 0x10]);
        bytes[0x20..0x24].copy_from_slice(&[0x80, 1, 0, 4]);
        let sum = crc16_ccitt_false(&bytes[..253]);
        bytes[253..].copy_from_slice(&sum.to_be_bytes());
        let sector = RawSector {
            id: 1,
            checksum_candidate: checksum_candidate(&bytes),
            bytes: bytes.to_vec(),
        };
        let decoded = decode_g102_format4_profile(&info, &sector).unwrap();
        assert_eq!(decoded.report_rate_raw, 1);
        assert_eq!(decoded.dpi_stages, [1000, 1600, 2200, 3200, 4200]);
        assert!(matches!(
            decoded.buttons[0].action,
            ButtonAction::MouseButton {
                mask: 4,
                label: Some("Middle")
            }
        ));
        let mut bad = sector;
        bad.bytes[253] ^= 1;
        assert!(decode_g102_format4_profile(&info, &bad).is_err());
    }

    #[test]
    fn backup_parser_rejects_truncation_trailing_bytes_and_bad_sector_size() {
        let backup = include_bytes!("../../g102-backup.obk");
        assert!(decode_backup(&backup[..backup.len() - 1]).is_err());
        let mut trailing = backup.to_vec();
        trailing.push(0);
        assert!(decode_backup(&trailing).is_err());
        let mut wrong = backup.to_vec();
        wrong[32] = 254;
        assert!(decode_backup(&wrong).is_err());
    }

    #[test]
    fn button_action_decode_and_encode_roundtrip() {
        // Mouse button
        let mouse_raw = [0x80, 0x01, 0x00, 0x08];
        let action = ButtonAction::from_bytes(mouse_raw);
        assert_eq!(
            action,
            ButtonAction::MouseButton {
                mask: 0x0008,
                label: Some("Back")
            }
        );
        assert!(action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), mouse_raw);

        // Keyboard
        let key_raw = [0x80, 0x02, 0x08, 0x2f];
        let action = ButtonAction::from_bytes(key_raw);
        assert_eq!(
            action,
            ButtonAction::Keyboard {
                modifiers: 0x08,
                key: 0x2f
            }
        );
        assert!(!action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), key_raw);

        // Consumer
        let cons_raw = [0x80, 0x03, 0x02, 0x24];
        let action = ButtonAction::from_bytes(cons_raw);
        assert_eq!(action, ButtonAction::Consumer { usage: 0x0224 });
        assert!(!action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), cons_raw);

        // Matching special-action bytes occur in the supplied backup;
        // runtime behavior remains unverified.
        let special_g102 = [0x90, 0x05, 0xff, 0xff];
        let action = ButtonAction::from_bytes(special_g102);
        assert!(matches!(
            action,
            ButtonAction::Special {
                code: 0x05,
                parameters: [0xff, 0xff],
                ..
            }
        ));
        assert!(action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), special_g102);

        // Other special-action encoding is not accepted by the offline editor.
        let special_unverified = [0x90, 0x03, 0x00, 0x00];
        let action = ButtonAction::from_bytes(special_unverified);
        assert!(!action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), special_unverified);

        // Macro
        let macro_raw = [0x01, 0x02, 0x03, 0x04];
        let action = ButtonAction::from_bytes(macro_raw);
        assert_eq!(
            action,
            ButtonAction::Macro {
                sector: 0x0102,
                address: 0x0304
            }
        );
        assert!(!action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), macro_raw);

        // Disabled
        let dis_raw = [0xff, 0xff, 0xff, 0xff];
        let action = ButtonAction::from_bytes(dis_raw);
        assert_eq!(action, ButtonAction::Disabled);
        assert!(action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), dis_raw);

        // Disabled (zeros)
        let zero_raw = [0x00, 0x00, 0x00, 0x00];
        let action = ButtonAction::from_bytes(zero_raw);
        assert_eq!(action, ButtonAction::Disabled);

        // Unknown
        let unk_raw = [0x77, 0x88, 0x99, 0xaa];
        let action = ButtonAction::from_bytes(unk_raw);
        assert_eq!(action, ButtonAction::Unknown(unk_raw));
        assert!(!action.is_corroborated_g102_encoding());
        assert_eq!(action.to_bytes(), unk_raw);
    }

    #[test]
    fn verified_action_parsing_and_rejections() {
        assert_eq!(
            VerifiedAction::parse("left").unwrap(),
            VerifiedAction::LeftClick
        );
        assert_eq!(
            VerifiedAction::parse("right-click").unwrap(),
            VerifiedAction::RightClick
        );
        assert_eq!(
            VerifiedAction::parse("middle").unwrap(),
            VerifiedAction::MiddleClick
        );
        assert_eq!(VerifiedAction::parse("back").unwrap(), VerifiedAction::Back);
        assert_eq!(
            VerifiedAction::parse("forward").unwrap(),
            VerifiedAction::Forward
        );
        assert_eq!(
            VerifiedAction::parse("cycle-dpi").unwrap(),
            VerifiedAction::DpiCycle
        );
        assert_eq!(
            VerifiedAction::parse("disable").unwrap(),
            VerifiedAction::Disabled
        );

        // Rejections of unverified / unsupported actions
        assert!(VerifiedAction::parse("key:08:2f").is_err());
        assert!(VerifiedAction::parse("consumer:0224").is_err());
        assert!(VerifiedAction::parse("macro:01:00").is_err());
        assert!(VerifiedAction::parse("dpi-up").is_err());
        assert!(VerifiedAction::parse("profile-cycle").is_err());
        assert!(VerifiedAction::parse("something-invalid").is_err());
    }

    #[test]
    fn remap_plan_modifies_only_specified_slot_and_crc() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let plan = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)]).unwrap();

        assert_eq!(plan.profile_sector_id, 1);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].slot, 4);
        assert_eq!(plan.changes[0].offset, 0x2c);
        assert_eq!(plan.changes[0].old_raw, [0x80, 0x01, 0x00, 0x08]);
        assert_eq!(plan.changes[0].new_raw, [0x80, 0x01, 0x00, 0x02]);
        assert_eq!(plan.old_checksum, 0xb3d1);
        assert_eq!(plan.new_checksum, 0xdd49);

        // Byte diffs: slot 4 low byte changed from 0x08 to 0x02, and CRC at 253, 254 changed
        assert_eq!(plan.byte_diffs.len(), 3);
        assert_eq!(
            plan.byte_diffs[0],
            ByteDiff {
                offset: 0x2f,
                old_byte: 0x08,
                new_byte: 0x02
            }
        );
        assert_eq!(
            plan.byte_diffs[1],
            ByteDiff {
                offset: 253,
                old_byte: 0xb3,
                new_byte: 0xdd
            }
        );
        assert_eq!(
            plan.byte_diffs[2],
            ByteDiff {
                offset: 254,
                old_byte: 0xd1,
                new_byte: 0x49
            }
        );

        // Verify that EVERY OTHER BYTE in the 255-byte profile sector is 100% UNCHANGED
        let orig_s1 = &backup.sectors[1].bytes;
        let mod_s1 = &plan.modified_backup.sectors[1].bytes;
        for i in 0..255 {
            if i == 0x2f || i == 253 || i == 254 {
                continue;
            }
            assert_eq!(
                orig_s1[i], mod_s1[i],
                "byte at offset 0x{i:02x} must not change"
            );
        }

        // Verify specific sensitive fields in sector 1 explicitly:
        // Report rate (offset 0)
        assert_eq!(mod_s1[0], orig_s1[0]);
        // Default & Shift DPI stage indices (offsets 1, 2)
        assert_eq!(&mod_s1[1..3], &orig_s1[1..3]);
        // 5 DPI stages (offsets 3..13)
        assert_eq!(&mod_s1[3..13], &orig_s1[3..13]);
        // Color (offsets 13..16)
        assert_eq!(&mod_s1[13..16], &orig_s1[13..16]);
        // Power mode and angle snap (offsets 16, 17)
        assert_eq!(&mod_s1[16..18], &orig_s1[16..18]);
        // Unmodified button slots:
        // Slot 1 (0x20..0x24)
        assert_eq!(&mod_s1[0x20..0x24], &orig_s1[0x20..0x24]);
        // Slot 2 (0x24..0x28)
        assert_eq!(&mod_s1[0x24..0x28], &orig_s1[0x24..0x28]);
        // Slot 3 (0x28..0x2c)
        assert_eq!(&mod_s1[0x28..0x2c], &orig_s1[0x28..0x2c]);
        // Slot 5 (0x30..0x34)
        assert_eq!(&mod_s1[0x30..0x34], &orig_s1[0x30..0x34]);
        // Slot 6 (0x34..0x38)
        assert_eq!(&mod_s1[0x34..0x38], &orig_s1[0x34..0x38]);
        // Unused button table region (0x38..0x60)
        assert_eq!(&mod_s1[0x38..0x60], &orig_s1[0x38..0x60]);
        // G-shift region (0x60..0xa0)
        assert_eq!(&mod_s1[0x60..0xa0], &orig_s1[0x60..0xa0]);
        // Profile name region (0xa0..0xd0)
        assert_eq!(&mod_s1[0xa0..0xd0], &orig_s1[0xa0..0xd0]);
        // Lighting / extras region (0xd0..253)
        assert_eq!(&mod_s1[0xd0..253], &orig_s1[0xd0..253]);
    }

    #[test]
    fn remap_plan_proves_all_other_sectors_and_headers_remain_unchanged() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let plan = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)]).unwrap();

        // Container headers and metadata must be identical
        assert_eq!(backup.info, plan.modified_backup.info);
        assert_eq!(backup.mode, plan.modified_backup.mode);
        assert_eq!(
            backup.current_profile_raw,
            plan.modified_backup.current_profile_raw
        );
        assert_eq!(backup.sectors.len(), plan.modified_backup.sectors.len());

        // Sector 0 (directory) must be identical
        assert_eq!(
            backup.sectors[0].bytes,
            plan.modified_backup.sectors[0].bytes
        );
        assert_eq!(
            backup.sectors[0].checksum_candidate,
            plan.modified_backup.sectors[0].checksum_candidate
        );

        // Sectors 2..15 (erased) must be identical
        for i in 2..16 {
            assert_eq!(
                backup.sectors[i].bytes,
                plan.modified_backup.sectors[i].bytes
            );
            assert_eq!(
                backup.sectors[i].checksum_candidate,
                plan.modified_backup.sectors[i].checksum_candidate
            );
        }
    }

    #[test]
    fn remap_plan_decodes_cleanly_with_new_assignment() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let plan = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)]).unwrap();

        // Modified profile sector decodes with decode_g102_format4_profile
        let decoded = decode_g102_format4_profile(
            &plan.modified_backup.info,
            &plan.modified_backup.sectors[1],
        )
        .unwrap();

        assert_eq!(decoded.checksum, 0xdd49);
        assert_eq!(decoded.report_rate_raw, 1);
        assert_eq!(decoded.default_dpi_stage, 1);
        assert_eq!(decoded.shifted_dpi_stage, 0);
        assert_eq!(decoded.dpi_stages, [1000, 1600, 2200, 3200, 4200]);
        assert_eq!(decoded.buttons.len(), 6);
        assert_eq!(decoded.buttons[0].raw, [0x80, 1, 0, 1]);
        assert_eq!(decoded.buttons[1].raw, [0x80, 1, 0, 2]);
        assert_eq!(decoded.buttons[2].raw, [0x80, 1, 0, 4]);
        // Slot 4 is now Right click
        assert_eq!(decoded.buttons[3].raw, [0x80, 1, 0, 2]);
        assert!(matches!(
            decoded.buttons[3].action,
            ButtonAction::MouseButton {
                mask: 2,
                label: Some("Right")
            }
        ));
        // Slot 5 is Forward
        assert_eq!(decoded.buttons[4].raw, [0x80, 1, 0, 16]);
        // Slot 6 is Cycle DPI
        assert_eq!(decoded.buttons[5].raw, [0x90, 5, 0xff, 0xff]);
    }

    #[test]
    fn multi_slot_remap_swaps_buttons_correctly() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        // Swap side buttons: slot 4 to Forward, slot 5 to Back
        let plan = create_button_remap_plan(
            &backup,
            &[(4, VerifiedAction::Forward), (5, VerifiedAction::Back)],
        )
        .unwrap();

        assert_eq!(plan.changes.len(), 2);
        assert_eq!(plan.changes[0].slot, 4);
        assert_eq!(plan.changes[0].new_raw, [0x80, 0x01, 0x00, 0x10]);
        assert_eq!(plan.changes[1].slot, 5);
        assert_eq!(plan.changes[1].new_raw, [0x80, 0x01, 0x00, 0x08]);

        let decoded = decode_g102_format4_profile(
            &plan.modified_backup.info,
            &plan.modified_backup.sectors[1],
        )
        .unwrap();

        assert_eq!(decoded.buttons[3].raw, [0x80, 1, 0, 16]);
        assert_eq!(decoded.buttons[4].raw, [0x80, 1, 0, 8]);
    }

    #[test]
    fn remap_plan_rejects_out_of_range_slot() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        assert!(create_button_remap_plan(&backup, &[(0, VerifiedAction::LeftClick)]).is_err());
        assert!(create_button_remap_plan(&backup, &[(7, VerifiedAction::LeftClick)]).is_err());
        assert!(create_button_remap_plan(&backup, &[]).is_err());
    }

    #[test]
    fn compare_backups_detects_differences() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let plan = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)]).unwrap();

        let diff = compare_backups(&backup, &plan.modified_backup);
        assert!(!diff.metadata_changed);
        assert_eq!(diff.sector_diffs.len(), 1);
        assert_eq!(diff.sector_diffs[0].0, 1);
        assert_eq!(diff.sector_diffs[0].1.len(), 3);

        // Comparing backup with itself yields no diffs
        let self_diff = compare_backups(&backup, &backup);
        assert!(!self_diff.metadata_changed);
        assert!(self_diff.sector_diffs.is_empty());
    }

    #[test]
    fn save_backup_file_refuses_to_overwrite_input_file() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let modified = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)])
            .unwrap()
            .modified_backup;
        let path =
            std::env::temp_dir().join(format!("g102_do_not_overwrite_{}.obk", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let original_bytes = encode_backup(
            &backup.info,
            backup.mode,
            backup.current_profile_raw,
            &backup.sectors,
        )
        .unwrap();
        std::fs::write(&path, &original_bytes).unwrap();

        let result = save_backup_file(path.to_str().unwrap(), &modified, path.to_str());
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
        let _ = std::fs::remove_file(path);
    }

    struct SimulatedG102Device {
        writes: Vec<Vec<u8>>,
        info: [u8; 11],
        mode: u8,
        current_profile: [u8; 3],
        sectors: Vec<Vec<u8>>,
        write_buffer: Vec<u8>,
        fail_at_fn: Option<u8>,
        timeout_at_fn: Option<u8>,
        disconnect_at_fn: Option<u8>,
        delay_at_fn: Option<(u8, Duration)>,
        fail_at_chunk_offset: Option<usize>,
        disconnect_at_chunk_offset: Option<usize>,
        corrupt_readback_offset: Option<usize>,
        committed: bool,
    }

    impl SimulatedG102Device {
        fn new_from_backup(backup: &RawBackup) -> Self {
            let sectors = backup.sectors.iter().map(|s| s.bytes.clone()).collect();
            Self {
                writes: Vec::new(),
                info: backup.info.raw,
                mode: 1, // Onboard
                current_profile: backup.current_profile_raw,
                sectors,
                write_buffer: Vec::new(),
                fail_at_fn: None,
                timeout_at_fn: None,
                disconnect_at_fn: None,
                delay_at_fn: None,
                fail_at_chunk_offset: None,
                disconnect_at_chunk_offset: None,
                corrupt_readback_offset: None,
                committed: false,
            }
        }

        fn apply_pending_commit(&mut self) {
            self.committed = true;
            if self.sectors.len() > 1 && self.write_buffer.len() >= 255 {
                self.sectors[1] = self.write_buffer[..255].to_vec();
            }
        }
    }

    impl HidTransport for SimulatedG102Device {
        fn write_report(&mut self, report: &[u8]) -> Result<(), String> {
            self.writes.push(report.to_vec());
            Ok(())
        }

        fn read_report(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, String> {
            let req_bytes = self.writes.last().ok_or("no write report")?;
            let req = Packet::decode(req_bytes).map_err(|e| e.to_string())?;

            if self.disconnect_at_fn == Some(req.function_id) {
                if req.function_id == FN_END_WRITE {
                    self.apply_pending_commit();
                }
                return Err("HID transport error: device disconnected".into());
            }

            if let Some(to_fn) = self.timeout_at_fn
                && req.function_id == to_fn
            {
                // Model the ambiguous case: the device applies the commit,
                // but the acknowledgement is lost or arrives too late.
                if req.function_id == FN_END_WRITE {
                    self.apply_pending_commit();
                }
                return Ok(None);
            }

            if let Some((del_fn, delay)) = self.delay_at_fn
                && req.function_id == del_fn
            {
                if delay > timeout {
                    return Ok(None);
                }
                std::thread::sleep(delay);
            }

            if let Some(fail_fn) = self.fail_at_fn
                && req.function_id == fail_fn
            {
                // Build a correctly-formed HID++ 2.0 error response directly
                // as raw bytes, bypassing Packet::new whose function/sw encoding
                // is not compatible with the error-packet wire format.
                //
                // HID++ 2.0 error packet layout (20-byte long report):
                //   [0]  = 0x11 (long report ID)
                //   [1]  = device_index
                //   [2]  = 0xff (error marker)
                //   [3]  = feature_index from failed request
                //   [4]  = (function_id << 4) | software_id from failed request
                //   [5]  = error code (0x02 = invalid argument)
                //   [6..19] = 0x00
                let mut raw = vec![0u8; 20];
                raw[0] = 0x11; // long report
                raw[1] = req.device_index;
                raw[2] = 0xff; // error marker
                raw[3] = req.feature_index;
                raw[4] = (req.function_id << 4) | req.software_id;
                raw[5] = 0x02; // invalid argument
                return Ok(Some(raw));
            }

            match req.function_id {
                FN_GET_INFO => {
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &self.info,
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                FN_GET_MODE => {
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &[self.mode],
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                FN_GET_CURRENT_PROFILE => {
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &self.current_profile,
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                FN_MEMORY_READ => {
                    let sector = u16::from_be_bytes([req.params[0], req.params[1]]) as usize;
                    let offset = u16::from_be_bytes([req.params[2], req.params[3]]) as usize;
                    let mut data = vec![0u8; 16];
                    if let Some(sec_bytes) = self.sectors.get(sector) {
                        let len = (sec_bytes.len().saturating_sub(offset)).min(16);
                        data[..len].copy_from_slice(&sec_bytes[offset..offset + len]);
                    }
                    if self.committed
                        && let Some(corrupt_off) = self.corrupt_readback_offset
                        && sector == 1
                        && (offset..offset + 16).contains(&corrupt_off)
                    {
                        data[corrupt_off - offset] ^= 0xff;
                    }
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &data,
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                FN_START_WRITE => {
                    let length = u16::from_be_bytes([req.params[4], req.params[5]]) as usize;
                    self.write_buffer = Vec::with_capacity(length);
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &[],
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                FN_WRITE_DATA => {
                    if let Some(disc_offset) = self.disconnect_at_chunk_offset
                        && self.write_buffer.len() == disc_offset
                    {
                        return Err("HID transport error: device disconnected".into());
                    }
                    if let Some(fail_offset) = self.fail_at_chunk_offset
                        && self.write_buffer.len() == fail_offset
                    {
                        // Build a correctly-formed HID++ 2.0 error response (see
                        // fail_at_fn block above for the wire format explanation).
                        let mut raw = vec![0u8; 20];
                        raw[0] = 0x11;
                        raw[1] = req.device_index;
                        raw[2] = 0xff;
                        raw[3] = req.feature_index;
                        raw[4] = (req.function_id << 4) | req.software_id;
                        raw[5] = 0x02; // invalid argument
                        return Ok(Some(raw));
                    }
                    self.write_buffer.extend_from_slice(&req.params[..16]);
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &[],
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                FN_END_WRITE => {
                    self.apply_pending_commit();
                    let reply = Packet::new(
                        ReportKind::Long,
                        req.device_index,
                        req.feature_index,
                        req.function_id,
                        req.software_id,
                        &[],
                    )
                    .unwrap();
                    Ok(Some(reply.encode()))
                }
                _ => Err(format!("unhandled function ID {}", req.function_id)),
            }
        }
    }

    #[test]
    fn test_write_sector_protocol_packets() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let test_sector = &backup.sectors[1].bytes;

        write_sector_with_tracking(&mut client, 0x0f, 1, test_sector, None, None).unwrap();
        let transport = client.into_transport();

        // 1 start_write + 16 write_chunks + 1 end_write = 18 packets
        assert_eq!(transport.writes.len(), 18);

        // Verify FN_START_WRITE packet (fn 6)
        let start_pkt = Packet::decode(&transport.writes[0]).unwrap();
        assert_eq!(start_pkt.feature_index, 0x0f);
        assert_eq!(start_pkt.function_id, FN_START_WRITE);
        assert_eq!(start_pkt.params[..6], [0, 1, 0, 0, 0, 255]);

        // Verify 16 FN_WRITE_DATA packets (fn 7)
        for i in 0..16 {
            let chunk_pkt = Packet::decode(&transport.writes[1 + i]).unwrap();
            assert_eq!(chunk_pkt.function_id, FN_WRITE_DATA);
            assert_eq!(chunk_pkt.params.len(), 16);
            if i < 15 {
                assert_eq!(chunk_pkt.params[..16], test_sector[i * 16..(i + 1) * 16]);
            } else {
                // Mock fixture explicitly supplies 0xff; this is not G102 evidence.
                assert_eq!(chunk_pkt.params[..15], test_sector[240..255]);
                assert_eq!(chunk_pkt.params[15], 0xff);
            }
        }

        // Verify FN_END_WRITE packet (fn 8)
        let end_pkt = Packet::decode(&transport.writes[17]).unwrap();
        assert_eq!(end_pkt.function_id, FN_END_WRITE);
        assert!(end_pkt.params.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_write_failure_injection_start_write() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.fail_at_fn = Some(FN_START_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();
        assert!(matches!(err, WriteProgressError::StartWriteFailed(_)));
        let transport = client.into_transport();
        assert_eq!(transport.writes.len(), 1);
    }

    #[test]
    fn test_write_failure_injection_chunk() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.fail_at_chunk_offset = Some(48); // Fail at 4th chunk (bytes 48..64)
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();
        match err {
            WriteProgressError::WriteChunkFailed { offset, .. } => {
                assert_eq!(offset, 48);
            }
            other => panic!("unexpected error: {:?}", other),
        }
        let transport = client.into_transport();
        // 1 start + 3 successful chunks + 1 failed chunk = 5 writes. No FN_END_WRITE!
        assert_eq!(transport.writes.len(), 5);
    }

    #[test]
    fn test_write_failure_injection_commit() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.fail_at_fn = Some(FN_END_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();
        assert!(matches!(
            err,
            WriteProgressError::CommitFailed {
                state: WriteTransactionState::PotentiallyCommitted,
                ambiguous_timeout: false,
                ..
            }
        ));
    }

    #[test]
    fn test_execute_remap_refuses_without_confirm_write() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            false,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, WriteError::SafetyRefusal(_)));
        let transport = client.into_transport();
        assert!(transport.writes.is_empty());
    }

    #[test]
    fn p8_public_write_apis_remain_disabled_even_with_confirmation() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let remap_error = super::execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            None,
            None,
        )
        .unwrap_err();
        assert!(remap_error.to_string().contains(PROFILE_WRITE_GATE_MESSAGE));

        let restore_error = super::execute_profile_restore(
            &mut client,
            0x0f,
            std::path::Path::new("does-not-need-to-exist.obk"),
            true,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            restore_error
                .to_string()
                .contains(PROFILE_WRITE_GATE_MESSAGE)
        );
        assert!(client.into_transport().writes.is_empty());
    }

    #[test]
    fn test_execute_remap_successful_with_mock() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let tmp_backup =
            std::env::temp_dir().join(format!("g102_test_remap_{}.obk", std::process::id()));
        let _ = std::fs::remove_file(&tmp_backup);

        let report = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            Some(&tmp_backup),
            None,
        )
        .unwrap();

        assert!(report.readback_verified);
        assert_eq!(report.state, WriteTransactionState::Verified);
        assert_eq!(report.sector_written, 1);
        assert_eq!(report.bytes_written, 255);
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0].slot, 4);
        assert!(matches!(
            report.changes[0].old_action,
            ButtonAction::MouseButton { mask: 8, .. }
        ));
        assert!(matches!(
            report.changes[0].new_action,
            ButtonAction::MouseButton { mask: 2, .. }
        ));

        // Verify pre-write backup was written and decodes cleanly
        assert!(tmp_backup.exists());
        let saved_bytes = std::fs::read(&tmp_backup).unwrap();
        let decoded_saved = decode_backup(&saved_bytes).unwrap();
        assert_eq!(decoded_saved.sectors[1].bytes, backup.sectors[1].bytes);

        // Verify simulated device flash now has the updated right-click button
        let sim_dev = client.into_transport();
        let decoded_new = decode_g102_format4_profile(
            &backup.info,
            &RawSector {
                id: 1,
                bytes: sim_dev.sectors[1].clone(),
                checksum_candidate: checksum_candidate(&sim_dev.sectors[1]),
            },
        )
        .unwrap();
        assert_eq!(
            decoded_new.buttons[3].action,
            ButtonAction::MouseButton {
                mask: 2,
                label: Some("Right")
            }
        );

        let _ = std::fs::remove_file(&tmp_backup);
    }

    #[test]
    fn test_execute_remap_detects_readback_byte_mismatch() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.corrupt_readback_offset = Some(0x2d); // corrupt button byte in readback
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let tmp_backup = std::env::temp_dir().join(format!(
            "g102_test_byte_mismatch_{}.obk",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp_backup);

        let err = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            Some(&tmp_backup),
            None,
        )
        .unwrap_err();

        match err {
            WriteError::Progress(WriteProgressError::ReadBackMismatch { offset, .. }) => {
                assert_eq!(offset, 0x2d);
            }
            other => panic!("unexpected error: {:?}", other),
        }
        let _ = std::fs::remove_file(&tmp_backup);
    }

    #[test]
    fn test_execute_remap_detects_readback_checksum_mismatch() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.corrupt_readback_offset = Some(253); // corrupt checksum byte in readback
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let tmp_backup =
            std::env::temp_dir().join(format!("g102_test_crc_mismatch_{}.obk", std::process::id()));
        let _ = std::fs::remove_file(&tmp_backup);

        let err = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            Some(&tmp_backup),
            None,
        )
        .unwrap_err();

        match err {
            WriteError::Progress(WriteProgressError::ReadBackMismatch { offset, .. }) => {
                assert_eq!(offset, 253);
            }
            other => panic!("unexpected error: {:?}", other),
        }
        let _ = std::fs::remove_file(&tmp_backup);
    }

    #[test]
    fn test_checksum_mismatch_display_and_conversion() {
        let backup_path = std::path::PathBuf::from("/tmp/test_backup.obk");
        let err = WriteProgressError::ChecksumMismatch {
            stored: 0x1234,
            calculated: 0x5678,
            backup_path: backup_path.clone(),
        };
        let msg = err.to_string();
        assert!(msg.contains("0x1234"));
        assert!(msg.contains("0x5678"));
        assert!(msg.contains("/tmp/test_backup.obk"));
        let write_err: WriteError = err.into();
        assert!(matches!(
            write_err,
            WriteError::Progress(WriteProgressError::ChecksumMismatch {
                stored: 0x1234,
                calculated: 0x5678,
                ..
            })
        ));
    }

    #[test]
    fn test_execute_restore_with_mock() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();

        // Create modified profile in simulated device
        let plan = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)]).unwrap();
        let preview = compare_backups(&plan.modified_backup, &backup);
        assert!(!preview.metadata_changed);
        assert_eq!(preview.sector_diffs.len(), 1);
        assert_eq!(preview.sector_diffs[0].0, 1);
        assert_eq!(
            preview.sector_diffs[0]
                .1
                .iter()
                .map(|diff| diff.offset)
                .collect::<Vec<_>>(),
            [0x2f, 253, 254]
        );
        assert_eq!(
            checksum_candidate(&plan.modified_backup.sectors[1].bytes),
            ChecksumCandidate::Crc16CcittFalseMatch(plan.new_checksum)
        );
        let sim = SimulatedG102Device::new_from_backup(&plan.modified_backup);
        assert_eq!(sim.sectors[1], plan.modified_backup.sectors[1].bytes);

        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let orig_file =
            std::env::temp_dir().join(format!("g102_test_orig_{}.obk", std::process::id()));
        let safety_file =
            std::env::temp_dir().join(format!("g102_test_safety_{}.obk", std::process::id()));
        let _ = std::fs::remove_file(&orig_file);
        let _ = std::fs::remove_file(&safety_file);

        // Write original backup to orig_file
        save_backup_file(orig_file.to_str().unwrap(), &backup, None).unwrap();

        let report = execute_profile_restore(
            &mut client,
            0x0f,
            &orig_file,
            true,
            Some(&safety_file),
            None,
        )
        .unwrap();

        assert!(report.readback_verified);
        assert_eq!(report.state, WriteTransactionState::Verified);
        assert_eq!(report.sectors_written, vec![1]);

        // The safety backup captures the modified pre-restore profile and CRC.
        let safety = decode_backup(&std::fs::read(&safety_file).unwrap()).unwrap();
        assert_eq!(
            safety.sectors[1].bytes,
            plan.modified_backup.sectors[1].bytes
        );
        assert_eq!(
            safety.sectors[1].checksum_candidate,
            ChecksumCandidate::Crc16CcittFalseMatch(plan.new_checksum)
        );

        // Safety backup of modified state was created
        assert!(safety_file.exists());

        // Simulated device now has original sector 1 bytes
        let sim_dev = client.into_transport();
        assert_eq!(sim_dev.sectors[1], backup.sectors[1].bytes);
        assert!(
            sim_dev
                .sectors
                .iter()
                .zip(&backup.sectors)
                .all(|(actual, expected)| actual == &expected.bytes)
        );
        assert_eq!(
            checksum_candidate(&sim_dev.sectors[1]),
            backup.sectors[1].checksum_candidate
        );
        assert_eq!(
            std::fs::read(&orig_file).unwrap(),
            encode_backup(
                &backup.info,
                backup.mode,
                backup.current_profile_raw,
                &backup.sectors
            )
            .unwrap(),
            "restore must not overwrite the original backup"
        );

        let _ = std::fs::remove_file(&orig_file);
        let _ = std::fs::remove_file(&safety_file);
    }

    #[test]
    fn restore_preview_from_mock_is_non_empty_and_read_only() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let modified = create_button_remap_plan(&backup, &[(4, VerifiedAction::RightClick)])
            .unwrap()
            .modified_backup;
        let sim = SimulatedG102Device::new_from_backup(&modified);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let info = read_info(&mut client, 0x0f).unwrap();
        let mode = read_mode(&mut client, 0x0f).unwrap();
        let current_profile_raw = read_current_profile_raw(&mut client, 0x0f).unwrap();
        let sectors = read_writable_memory(&mut client, 0x0f, &info).unwrap();
        let current = RawBackup {
            info,
            mode,
            current_profile_raw,
            sectors,
        };

        let preview = compare_backups(&current, &backup);
        assert!(!preview.metadata_changed);
        assert_eq!(preview.sector_diffs.len(), 1);
        assert_eq!(preview.sector_diffs[0].0, 1);
        assert_eq!(
            preview.sector_diffs[0]
                .1
                .iter()
                .map(|diff| diff.offset)
                .collect::<Vec<_>>(),
            [0x2f, 253, 254]
        );
        assert_eq!(
            checksum_candidate(&current.sectors[1].bytes),
            modified.sectors[1].checksum_candidate
        );

        let transport = client.into_transport();
        assert!(transport.writes.iter().all(|report| {
            Packet::decode(report)
                .map(|packet| {
                    !matches!(
                        packet.function_id,
                        FN_START_WRITE | FN_WRITE_DATA | FN_END_WRITE
                    )
                })
                .unwrap_or(false)
        }));
    }

    #[test]
    fn restore_rejects_malformed_backup_before_hid_access() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let malformed_path =
            std::env::temp_dir().join(format!("g102_malformed_restore_{}.obk", std::process::id()));
        std::fs::write(&malformed_path, b"not a G102OBK1 backup").unwrap();

        let result = execute_profile_restore(&mut client, 0x0f, &malformed_path, true, None, None);
        assert!(matches!(result, Err(WriteError::InvalidBackup(_))));
        assert!(client.into_transport().writes.is_empty());
        let _ = std::fs::remove_file(malformed_path);
    }

    #[test]
    fn restore_rejects_invalid_profile_checksum_before_hid_access() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut invalid = backup.clone();
        invalid.sectors[1].bytes[0x20] ^= 0x01;
        invalid.sectors[1].checksum_candidate = checksum_candidate(&invalid.sectors[1].bytes);
        let path =
            std::env::temp_dir().join(format!("g102_bad_crc_restore_{}.obk", std::process::id()));
        save_backup_file(path.to_str().unwrap(), &invalid, None).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let result = execute_profile_restore(&mut client, 0x0f, &path, true, None, None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("CRC-16/CCITT-FALSE")
        );
        assert!(client.into_transport().writes.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn restore_rejects_changes_to_unsupported_sectors_before_write() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut invalid = backup.clone();
        invalid.sectors[2].bytes[0] = 0;
        invalid.sectors[2].checksum_candidate = checksum_candidate(&invalid.sectors[2].bytes);
        let path = std::env::temp_dir().join(format!(
            "g102_unsupported_sector_restore_{}.obk",
            std::process::id()
        ));
        save_backup_file(path.to_str().unwrap(), &invalid, None).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let result = execute_profile_restore(&mut client, 0x0f, &path, true, None, None);
        assert!(matches!(result, Err(WriteError::SafetyRefusal(_))));
        let transport = client.into_transport();
        assert!(transport.writes.iter().all(|report| {
            Packet::decode(report)
                .map(|packet| {
                    !matches!(
                        packet.function_id,
                        FN_START_WRITE | FN_WRITE_DATA | FN_END_WRITE
                    )
                })
                .unwrap_or(false)
        }));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_explicit_simulated_tail_byte_is_sent_verbatim() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let test_sector = &backup.sectors[1].bytes;
        assert_eq!(test_sector.len(), 255);

        write_sector_with_tracking(&mut client, 0x0f, 1, test_sector, None, None).unwrap();
        let transport = client.into_transport();

        // Chunk index 15 is the 16th chunk (index 16 in writes after FN_START_WRITE)
        let chunk15 = Packet::decode(&transport.writes[16]).unwrap();
        assert_eq!(chunk15.function_id, FN_WRITE_DATA);
        assert_eq!(chunk15.params.len(), 16);
        // Bytes 0..15 contain the last 15 bytes of sector data
        assert_eq!(&chunk15.params[..15], &test_sector[240..255]);
        // Byte 15 is the caller-selected mock byte, not a protocol default.
        assert_eq!(chunk15.params[15], 0xff);
    }

    #[test]
    fn test_commit_ambiguous_timeout_reported_with_recovery_guidance() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.timeout_at_fn = Some(FN_END_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let tmp_backup = std::env::temp_dir().join(format!(
            "g102_test_ambig_timeout_{}.obk",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp_backup);

        let err = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            Some(&tmp_backup),
            None,
        )
        .unwrap_err();

        match &err {
            WriteError::Progress(WriteProgressError::CommitFailed {
                ambiguous_timeout,
                timeout,
                backup_path,
                state,
                ..
            }) => {
                assert!(ambiguous_timeout);
                assert_eq!(*timeout, Duration::from_millis(2000));
                assert_eq!(backup_path, &tmp_backup);
                assert_eq!(*state, WriteTransactionState::PotentiallyCommitted);
            }
            other => panic!(
                "expected CommitFailed with ambiguous_timeout, got {:?}",
                other
            ),
        }

        // Verify the formatted error message includes crucial recovery warnings
        let msg = err.to_string();
        assert!(msg.contains("POTENTIALLY COMMITTED"));
        assert!(msg.contains("Do not retry, restore, or issue any other write"));
        assert!(msg.contains("READ-ONLY RECOVERY:"));
        assert!(msg.contains("g102-control onboard export after-commit.obk"));
        assert!(msg.contains(&format!(
            "g102-control onboard diff {} after-commit.obk",
            tmp_backup.display()
        )));
        assert!(!msg.contains("onboard restore"));
        assert!(client.into_transport().committed);

        let _ = std::fs::remove_file(&tmp_backup);
    }

    #[test]
    fn test_delayed_response_within_extended_timeout_succeeds() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        // Simulate an acknowledgement delayed but still within the deadline.
        sim.delay_at_fn = Some((FN_END_WRITE, Duration::from_millis(12)));
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        // With the default 2000ms commit timeout, this delayed acknowledgement succeeds.
        let test_sector = &backup.sectors[1].bytes;
        let res = write_sector_with_tracking(&mut client, 0x0f, 1, test_sector, None, None);
        assert!(res.is_ok());

        // A shorter operation-specific timeout reports an ambiguous outcome.
        let sim2 = SimulatedG102Device::new_from_backup(&backup);
        let mut client2 = HidppClient::new(sim2, 0xff, ReportKind::Long);
        client2.transport_mut().delay_at_fn = Some((FN_END_WRITE, Duration::from_millis(12)));
        let tight_opts = WriteOptions {
            commit_timeout: Duration::from_millis(8),
            ..Default::default()
        };
        let res2 =
            write_sector_with_tracking(&mut client2, 0x0f, 1, test_sector, Some(&tight_opts), None);
        match res2 {
            Err(WriteProgressError::CommitFailed {
                ambiguous_timeout: true,
                timeout,
                ..
            }) => {
                assert_eq!(timeout, Duration::from_millis(8));
            }
            other => panic!("expected ambiguous timeout, got {:?}", other),
        }
    }

    #[test]
    fn test_device_disconnection_mid_write_is_trapped() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        // Simulate physical USB disconnection at byte offset 32 (3rd chunk)
        sim.disconnect_at_chunk_offset = Some(32);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();

        match &err {
            WriteProgressError::WriteChunkFailed { offset, error } => {
                assert_eq!(*offset, 32);
                assert!(error.to_string().contains("device disconnected"));
            }
            other => panic!("expected WriteChunkFailed, got {:?}", other),
        }

        let msg = err.to_string();
        assert!(msg.contains("UNCOMMITTED"));
        assert!(msg.contains("No commit request was sent"));

        // Device transport should have only received 1 start + 2 chunks + 1 failed chunk (4 writes)
        // and crucially NEVER received FN_END_WRITE!
        let transport = client.into_transport();
        assert_eq!(transport.writes.len(), 4);
        assert!(!transport.committed);
    }

    #[test]
    fn test_write_options_custom_timeout_and_padding() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let test_sector = &backup.sectors[1].bytes;

        let custom_opts = WriteOptions {
            commit_timeout: Duration::from_millis(3500),
            tail_padding_byte: Some(0xaa),
        };

        write_sector_with_tracking(&mut client, 0x0f, 1, test_sector, Some(&custom_opts), None)
            .unwrap();
        let transport = client.into_transport();

        let chunk15 = Packet::decode(&transport.writes[16]).unwrap();
        assert_eq!(chunk15.params[15], 0xaa);
    }

    #[test]
    fn test_unique_backup_path_generation() {
        let temp_dir = TestTempDir::new();
        let path1 = generate_unique_backup_path_with_exists("test_prefix", |candidate| {
            temp_dir.path().join(candidate).exists()
        });
        assert!(path1.to_str().unwrap().ends_with(".obk"));

        // Create the candidate inside the isolated directory so the next call detects it.
        std::fs::write(temp_dir.path().join(&path1), b"test").unwrap();
        let path2 = generate_unique_backup_path_with_exists("test_prefix", |candidate| {
            temp_dir.path().join(candidate).exists()
        });
        assert_ne!(path1, path2);
        assert!(path2.to_str().unwrap().ends_with("_1.obk"));
    }

    // ── Phase 8: Protocol hardening tests ────────────────────────────────────

    /// Unknown G102 final-byte behavior must fail before FN_START_WRITE.
    #[test]
    fn p8_255_byte_sector_refuses_default_tail_before_start_write() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        assert_eq!(
            backup.sectors[1].bytes.len(),
            255,
            "sector must be 255 bytes to exercise the 15-byte tail chunk"
        );
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let sector_bytes = &backup.sectors[1].bytes;

        let error =
            super::write_sector_with_tracking(&mut client, 0x0f, 1, sector_bytes, None, None)
                .unwrap_err();
        assert!(matches!(error, WriteProgressError::PreflightRefused(_)));
        assert!(
            error
                .to_string()
                .contains("G102-compatible handling is unverified")
        );
        let transport = client.into_transport();
        assert!(
            transport.writes.is_empty(),
            "preflight must precede FN_START_WRITE"
        );
    }

    #[test]
    fn p8_aligned_transfer_needs_no_final_byte_policy() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let aligned = [0x5a; 32];

        super::write_sector_with_tracking(&mut client, 0x0f, 1, &aligned, None, None).unwrap();
        let transport = client.into_transport();
        assert_eq!(transport.writes.len(), 4); // start, two chunks, end
        for index in 1..=2 {
            let packet = Packet::decode(&transport.writes[index]).unwrap();
            assert_eq!(packet.function_id, FN_WRITE_DATA);
            assert_eq!(packet.params, [0x5a; 16]);
        }
    }

    #[test]
    fn p8_zero_commit_timeout_refuses_before_write_window() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let options = WriteOptions {
            commit_timeout: Duration::ZERO,
            tail_padding_byte: Some(0xff),
        };

        let error = super::write_sector_with_tracking(
            &mut client,
            0x0f,
            1,
            &backup.sectors[1].bytes,
            Some(&options),
            None,
        )
        .unwrap_err();
        assert!(matches!(error, WriteProgressError::PreflightRefused(_)));
        assert!(client.into_transport().writes.is_empty());
    }

    /// Confirm that a custom `tail_padding_byte` is used for the final chunk,
    /// and that all full chunks before it are unaffected.
    #[test]
    fn p8_custom_tail_padding_byte_affects_only_the_final_incomplete_chunk() {
        const CUSTOM_PAD: u8 = 0xaa;
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sim = SimulatedG102Device::new_from_backup(&backup);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
        let sector_bytes = &backup.sectors[1].bytes;

        let opts = WriteOptions {
            tail_padding_byte: Some(CUSTOM_PAD),
            ..Default::default()
        };
        write_sector_with_tracking(&mut client, 0x0f, 1, sector_bytes, Some(&opts), None).unwrap();
        let transport = client.into_transport();

        // All full chunks (0..15, indices 1..=15 in writes) must be unaffected
        for i in 0..15 {
            let pkt = Packet::decode(&transport.writes[1 + i]).unwrap();
            assert_eq!(pkt.params.len(), 16);
            assert_eq!(
                &pkt.params[..16],
                &sector_bytes[i * 16..(i + 1) * 16],
                "full chunk {i} must equal unmodified sector bytes"
            );
        }
        // Final chunk: data unchanged, padding byte is CUSTOM_PAD
        let last_pkt = Packet::decode(&transport.writes[16]).unwrap();
        assert_eq!(&last_pkt.params[..15], &sector_bytes[240..255]);
        assert_eq!(last_pkt.params[15], CUSTOM_PAD);
    }

    /// Verify that `WriteTransactionState` is correctly set to `Uncommitted`
    /// when `FN_START_WRITE` is rejected (flash was never written).
    #[test]
    fn p8_transaction_state_is_uncommitted_on_start_write_failure() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.fail_at_fn = Some(FN_START_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();

        // StartWriteFailed maps to Uncommitted state
        assert!(matches!(err, WriteProgressError::StartWriteFailed(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("UNCOMMITTED"),
            "StartWriteFailed must report UNCOMMITTED state"
        );
    }

    /// Verify that `WriteTransactionState` is `Uncommitted` when a chunk fails
    /// (no FN_END_WRITE was ever sent).
    #[test]
    fn p8_transaction_state_is_uncommitted_on_chunk_failure() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.fail_at_chunk_offset = Some(32);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();

        // WriteChunkFailed maps to Uncommitted state
        assert!(matches!(
            err,
            WriteProgressError::WriteChunkFailed { offset: 32, .. }
        ));
        let msg = err.to_string();
        assert!(
            msg.contains("UNCOMMITTED"),
            "WriteChunkFailed must report UNCOMMITTED state"
        );
        assert!(msg.contains("No commit request was sent"));

        // Critically: FN_END_WRITE must NEVER have been sent
        assert!(
            !client.into_transport().committed,
            "device must not have committed after chunk failure"
        );
    }

    /// Verify that `WriteTransactionState::PotentiallyCommitted` is returned
    /// when `FN_END_WRITE` times out, and that the error message forbids retries
    /// and includes recovery instructions with the backup path.
    #[test]
    fn p8_transaction_state_is_potentially_committed_on_end_write_timeout() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.timeout_at_fn = Some(FN_END_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let backup_sentinel = std::path::Path::new("/tmp/p8_sentinel_backup.obk");
        let err = write_sector_with_tracking(
            &mut client,
            0x0f,
            1,
            &backup.sectors[1].bytes,
            None,
            Some(backup_sentinel),
        )
        .unwrap_err();

        match &err {
            WriteProgressError::CommitFailed {
                state,
                ambiguous_timeout,
                backup_path,
                ..
            } => {
                assert_eq!(
                    *state,
                    WriteTransactionState::PotentiallyCommitted,
                    "timeout on FN_END_WRITE must yield PotentiallyCommitted"
                );
                assert!(
                    *ambiguous_timeout,
                    "ambiguous_timeout must be true for a timeout"
                );
                assert_eq!(backup_path, backup_sentinel);
            }
            other => panic!("expected CommitFailed, got {:?}", other),
        }

        let msg = err.to_string();
        assert!(msg.contains("POTENTIALLY COMMITTED"));
        assert!(msg.contains("Do not retry, restore, or issue any other write"));
        assert!(msg.contains("READ-ONLY RECOVERY:"));
        assert!(msg.contains("g102-control onboard export after-commit.obk"));
        assert!(
            msg.contains("g102-control onboard diff /tmp/p8_sentinel_backup.obk after-commit.obk")
        );
        assert!(msg.contains("/tmp/p8_sentinel_backup.obk"));
        assert!(!msg.contains("onboard restore"));
        assert!(client.into_transport().committed);
    }

    /// A matched device error after attempting `FN_END_WRITE` is potentially
    /// committed because rejection semantics are not validated on G102.
    #[test]
    fn p8_transaction_state_is_potentially_committed_on_end_write_device_error() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.fail_at_fn = Some(FN_END_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();

        match &err {
            WriteProgressError::CommitFailed {
                state,
                ambiguous_timeout,
                ..
            } => {
                assert!(
                    !ambiguous_timeout,
                    "a device error reply is not a timeout — ambiguous_timeout must be false"
                );
                assert_eq!(
                    *state,
                    WriteTransactionState::PotentiallyCommitted,
                    "commit-error semantics are unverified, so conservatively ambiguous"
                );
            }
            other => panic!("expected CommitFailed, got {:?}", other),
        }

        // Device must not have been committed in the simulated state
        assert!(
            !client.into_transport().committed,
            "device must not mark committed when FN_END_WRITE was rejected"
        );
    }

    #[test]
    fn p8_disconnect_after_commit_request_is_ambiguous_and_never_retried() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.disconnect_at_fn = Some(FN_END_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let error = write_sector_with_tracking(
            &mut client,
            0x0f,
            1,
            &backup.sectors[1].bytes,
            None,
            Some(std::path::Path::new("pre-write.obk")),
        )
        .unwrap_err();
        assert_eq!(
            error.transaction_state(),
            WriteTransactionState::PotentiallyCommitted
        );
        assert!(!matches!(
            &error,
            WriteProgressError::CommitFailed {
                ambiguous_timeout: true,
                ..
            }
        ));

        let transport = client.into_transport();
        assert!(transport.committed, "simulates commit before disconnect");
        assert_eq!(
            transport
                .writes
                .iter()
                .filter(|packet| Packet::decode(packet)
                    .is_ok_and(|packet| packet.function_id == FN_END_WRITE))
                .count(),
            1
        );
        assert_eq!(
            transport
                .writes
                .iter()
                .filter(|packet| Packet::decode(packet)
                    .is_ok_and(|packet| packet.function_id == FN_START_WRITE))
                .count(),
            1
        );
    }

    /// Verify that a commit timeout during `execute_profile_button_remap` does
    /// NOT trigger any further write operations (no retry, no second commit).
    #[test]
    fn p8_ambiguous_commit_never_retries_or_issues_a_second_write() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        sim.timeout_at_fn = Some(FN_END_WRITE);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let tmp = std::env::temp_dir().join(format!("g102_p8_no_retry_{}.obk", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let err = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            Some(&tmp),
            None,
        )
        .unwrap_err();

        // Confirm it's an ambiguous-commit error
        assert!(
            matches!(
                &err,
                WriteError::Progress(WriteProgressError::CommitFailed {
                    state: WriteTransactionState::PotentiallyCommitted,
                    ambiguous_timeout: true,
                    ..
                })
            ),
            "unexpected error: {:?}",
            err
        );

        // Count FN_END_WRITE packets — there must be exactly one
        let transport = client.into_transport();
        let end_write_count = transport
            .writes
            .iter()
            .filter(|raw| {
                Packet::decode(raw)
                    .ok()
                    .map(|p| p.function_id == FN_END_WRITE)
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            end_write_count, 1,
            "exactly one FN_END_WRITE must be sent — no retries"
        );

        // Count FN_START_WRITE packets — there must also be exactly one
        let start_write_count = transport
            .writes
            .iter()
            .filter(|raw| {
                Packet::decode(raw)
                    .ok()
                    .map(|p| p.function_id == FN_START_WRITE)
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            start_write_count, 1,
            "exactly one FN_START_WRITE — no second write cycle attempted"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    /// Verify that a USB disconnect mid-chunk yields `WriteChunkFailed` with
    /// `Uncommitted` state, and that no `FN_END_WRITE` is ever sent.
    #[test]
    fn p8_disconnect_mid_chunk_is_uncommitted_and_no_commit_sent() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        // Disconnect at byte offset 48 (after 3 chunks of 16 bytes each)
        sim.disconnect_at_chunk_offset = Some(48);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let err =
            write_sector_with_tracking(&mut client, 0x0f, 1, &backup.sectors[1].bytes, None, None)
                .unwrap_err();

        match &err {
            WriteProgressError::WriteChunkFailed { offset, error } => {
                assert_eq!(*offset, 48);
                assert!(
                    error.to_string().contains("device disconnected")
                        || error.to_string().contains("transport"),
                    "error should mention transport failure: {error}"
                );
            }
            other => panic!("expected WriteChunkFailed, got {:?}", other),
        }
        let msg = err.to_string();
        assert!(
            msg.contains("UNCOMMITTED"),
            "disconnect must yield UNCOMMITTED state"
        );

        let transport = client.into_transport();
        // No FN_END_WRITE should have been sent after a disconnect
        let commit_count = transport
            .writes
            .iter()
            .filter(|raw| {
                Packet::decode(raw)
                    .ok()
                    .map(|p| p.function_id == FN_END_WRITE)
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            commit_count, 0,
            "FN_END_WRITE must not be sent after disconnect"
        );
        assert!(
            !transport.committed,
            "device must not be in committed state"
        );
    }

    /// Verify that `WriteTransactionState::Committed` is correctly represented
    /// in readback-failure error messages (flash was written but verify failed).
    #[test]
    fn p8_transaction_state_is_committed_on_readback_failure() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let mut sim = SimulatedG102Device::new_from_backup(&backup);
        // Corrupt a button byte in the simulated device's readback
        sim.corrupt_readback_offset = Some(0x2d);
        let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);

        let tmp = std::env::temp_dir().join(format!(
            "g102_p8_committed_state_{}.obk",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);

        let err = execute_profile_button_remap(
            &mut client,
            0x0f,
            &[(4, VerifiedAction::RightClick)],
            true,
            Some(&tmp),
            None,
        )
        .unwrap_err();

        // Readback mismatch means the commit completed but the data doesn't match
        let msg = err.to_string();
        assert!(
            msg.contains("COMMITTED"),
            "readback mismatch must report COMMITTED state; got: {msg}"
        );
        assert!(msg.contains("onboard export") && msg.contains("onboard diff"));
        assert!(!msg.contains("onboard restore"));
        if let WriteError::Progress(progress) = &err {
            assert_eq!(
                progress.transaction_state(),
                WriteTransactionState::Committed
            );
        }

        let _ = std::fs::remove_file(&tmp);
    }

    /// Verify that `WriteOptions::commit_timeout` is passed to `FN_END_WRITE`
    /// and distinguishes a delayed response that falls within the extended
    /// timeout from one that falls outside the tight timeout.
    #[test]
    fn p8_commit_timeout_is_operation_specific_and_configurable() {
        let backup = decode_backup(include_bytes!("../../g102-backup.obk")).unwrap();
        let sector_bytes = &backup.sectors[1].bytes;

        // 1. A response delayed within the 2 000 ms deadline succeeds.
        {
            let mut sim = SimulatedG102Device::new_from_backup(&backup);
            sim.delay_at_fn = Some((FN_END_WRITE, Duration::from_millis(50)));
            let mut client = HidppClient::new(sim, 0xff, ReportKind::Long);
            let opts = WriteOptions {
                commit_timeout: Duration::from_millis(2000),
                ..Default::default()
            };
            let res =
                write_sector_with_tracking(&mut client, 0x0f, 1, sector_bytes, Some(&opts), None);
            assert!(
                res.is_ok(),
                "response within configured deadline must succeed"
            );
        }

        // 2. A response delayed beyond the configured deadline is ambiguous.
        {
            let mut sim2 = SimulatedG102Device::new_from_backup(&backup);
            sim2.delay_at_fn = Some((FN_END_WRITE, Duration::from_millis(50)));
            let mut client2 = HidppClient::new(sim2, 0xff, ReportKind::Long);
            let tight = WriteOptions {
                commit_timeout: Duration::from_millis(30),
                ..Default::default()
            };
            let res2 =
                write_sector_with_tracking(&mut client2, 0x0f, 1, sector_bytes, Some(&tight), None);
            match res2 {
                Err(WriteProgressError::CommitFailed {
                    state: WriteTransactionState::PotentiallyCommitted,
                    ambiguous_timeout: true,
                    timeout,
                    ..
                }) => {
                    assert_eq!(timeout, Duration::from_millis(30));
                }
                other => panic!("expected PotentiallyCommitted, got {:?}", other),
            }
        }
    }

    /// Verify that the error message for `PotentiallyCommitted` does not
    /// include any instruction that could be misread as authorizing a retry.
    #[test]
    fn p8_ambiguous_timeout_error_message_has_no_retry_instruction() {
        let err = WriteProgressError::CommitFailed {
            error: FeatureError::Client(crate::error::ClientError::Protocol(
                crate::error::ProtocolError::Timeout,
            )),
            state: WriteTransactionState::PotentiallyCommitted,
            ambiguous_timeout: true,
            timeout: DEFAULT_COMMIT_TIMEOUT,
            backup_path: std::path::PathBuf::from("/tmp/sentinel.obk"),
        };
        let msg = err.to_string();

        assert!(msg.contains("POTENTIALLY COMMITTED"));
        assert!(msg.contains("Do not retry, restore, or issue any other write"));
        assert!(msg.contains("READ-ONLY RECOVERY:"));
        assert!(msg.contains("2s"));
        assert!(msg.contains("/tmp/sentinel.obk"));
        // Must not contain misleading retry language
        assert!(
            !msg.to_lowercase().contains("try again"),
            "must not say 'try again'"
        );
        assert!(!msg.to_lowercase().contains("re-run the write"));
        assert!(!msg.contains("onboard restore"));
    }

    /// Verify that the `WriteTransactionState` display strings match the
    /// documented labels used in error message assertions.
    #[test]
    fn p8_write_transaction_state_display_matches_documented_labels() {
        assert_eq!(
            WriteTransactionState::Uncommitted.to_string(),
            "UNCOMMITTED"
        );
        assert_eq!(
            WriteTransactionState::PotentiallyCommitted.to_string(),
            "POTENTIALLY COMMITTED"
        );
        assert_eq!(WriteTransactionState::Committed.to_string(), "COMMITTED");
        assert_eq!(WriteTransactionState::Verified.to_string(), "VERIFIED");
    }

    /// Verify that `WriteOptions::default()` uses the documented safe constants.
    #[test]
    fn p8_write_options_defaults_match_documented_constants() {
        let opts = WriteOptions::default();
        assert_eq!(
            opts.commit_timeout, DEFAULT_COMMIT_TIMEOUT,
            "default commit_timeout must equal DEFAULT_COMMIT_TIMEOUT"
        );
        assert_eq!(opts.tail_padding_byte, None);
    }
}
