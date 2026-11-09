//! Optional, versioned user configuration. Loading a config never accesses HID.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub version: u32,
    pub device: DeviceConfig,
    pub dpi: DpiConfig,
    pub rate: RateConfig,
    pub rgb: RgbConfig,
    pub wallust: WallustConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            device: DeviceConfig::default(),
            dpi: DpiConfig::default(),
            rate: RateConfig::default(),
            rgb: RgbConfig::default(),
            wallust: WallustConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeviceConfig {
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub interface_number: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DpiConfig {
    pub sensor: u8,
    pub value: Option<u16>,
}

impl Default for DpiConfig {
    fn default() -> Self {
        Self {
            sensor: 0,
            value: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateConfig {
    pub hz: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RgbConfig {
    pub cluster: Option<u8>,
    pub effect_index: Option<u8>,
    pub color: Option<String>,
    pub period_ms: u16,
    pub brightness: u8,
    pub direction: String,
}

impl Default for RgbConfig {
    fn default() -> Self {
        Self {
            cluster: None,
            effect_index: None,
            color: None,
            period_ms: 3000,
            brightness: 100,
            direction: "right".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WallustConfig {
    pub enabled: bool,
    pub palette_file: Option<String>,
    pub color_key: String,
    pub cluster: Option<u8>,
    pub effect_index: Option<u8>,
}

impl Default for WallustConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            palette_file: None,
            color_key: "accent".into(),
            cluster: None,
            effect_index: None,
        }
    }
}

impl AppConfig {
    pub fn load(explicit_path: Option<&Path>, disabled: bool) -> Result<Self, String> {
        if disabled && explicit_path.is_some() {
            return Err("--config and --no-config cannot be used together".into());
        }
        if disabled {
            return Ok(Self::default());
        }

        let path = match explicit_path {
            Some(path) => Some(path.to_path_buf()),
            None => default_config_path(),
        };
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error)
                if explicit_path.is_none() && error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(format!(
                    "could not read config `{}`: {error}",
                    path.display()
                ));
            }
        };
        let config: Self = toml::from_str(&contents)
            .map_err(|error| format!("invalid config `{}`: {error}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != CONFIG_VERSION {
            return Err(format!(
                "unsupported config version {}; this build supports version {}",
                self.version, CONFIG_VERSION
            ));
        }
        if self.device.vendor_id.is_some() != self.device.product_id.is_some() {
            return Err("preferred device requires both `vendor_id` and `product_id`".into());
        }
        if self.device.interface_number.is_some() && self.device.vendor_id.is_none() {
            return Err(
                "preferred device `interface_number` requires both `vendor_id` and `product_id`"
                    .into(),
            );
        }
        if self
            .device
            .interface_number
            .is_some_and(|number| number < 0)
        {
            return Err("preferred device `interface_number` must be non-negative".into());
        }
        if self.dpi.value == Some(0) {
            return Err("configured DPI value must be in 1..=65535".into());
        }
        if self.rate.hz.is_some_and(|hz| !(1..=1000).contains(&hz)) {
            return Err("configured polling rate must be in 1..=1000 Hz".into());
        }
        if self.rgb.cluster.is_some_and(|cluster| cluster > 31) {
            return Err("configured RGB cluster must be in 0..=31".into());
        }
        if self.rgb.effect_index.is_some_and(|effect| effect > 63) {
            return Err("configured RGB effect index must be in 0..=63".into());
        }
        if let Some(color) = &self.rgb.color {
            validate_color(color).map_err(|_| {
                "configured RGB color must be six hexadecimal digits (RRGGBB)".to_owned()
            })?;
        }
        if !(1000..=8000).contains(&self.rgb.period_ms) {
            return Err("configured RGB period must be 1000..=8000 ms".into());
        }
        if !(1..=100).contains(&self.rgb.brightness) {
            return Err("configured RGB brightness must be 1..=100".into());
        }
        if !matches!(self.rgb.direction.as_str(), "left" | "right") {
            return Err("configured RGB direction must be `left` or `right`".into());
        }
        if self.wallust.cluster.is_some_and(|cluster| cluster > 31) {
            return Err("configured Wallust cluster must be in 0..=31".into());
        }
        if self.wallust.effect_index.is_some_and(|effect| effect > 63) {
            return Err("configured Wallust effect index must be in 0..=63".into());
        }
        if !matches!(
            self.wallust.color_key.as_str(),
            "background" | "foreground" | "accent" | "secondary"
        ) {
            return Err(
                "Wallust color_key must be `background`, `foreground`, `accent`, or `secondary`"
                    .into(),
            );
        }
        if self
            .wallust
            .palette_file
            .as_ref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err("Wallust palette_file must not be empty".into());
        }
        Ok(())
    }
}

pub fn default_config_path() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg);
        if path.is_absolute() {
            return Some(path.join("g102-control").join(CONFIG_FILE_NAME));
        }
    }
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("g102-control")
            .join(CONFIG_FILE_NAME)
    })
}

pub fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return env::var_os("HOME").map_or_else(|| PathBuf::from(path), PathBuf::from);
    }
    if let Some(relative) = path.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return Path::new(&home).join(relative);
        }
    }
    PathBuf::from(path)
}

pub fn validate_color(value: &str) -> Result<[u8; 3], String> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("color must be six hexadecimal digits (RRGGBB)".into());
    }
    Ok([
        u8::from_str_radix(&hex[0..2], 16).map_err(|_| "invalid red color component")?,
        u8::from_str_radix(&hex[2..4], 16).map_err(|_| "invalid green color component")?,
        u8::from_str_radix(&hex[4..6], 16).map_err(|_| "invalid blue color component")?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_file(contents: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "g102-control-config-{}-{id}.toml",
            std::process::id()
        ));
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn defaults_are_sensible_and_explicit_missing_config_errors() {
        let config = AppConfig::default();
        assert_eq!(config.version, CONFIG_VERSION);
        assert_eq!(config.dpi.sensor, 0);
        assert_eq!(config.rgb.period_ms, 3000);
        assert!(!config.wallust.enabled);
        assert!(
            AppConfig::load(Some(Path::new("/definitely/missing/config.toml")), false)
                .unwrap_err()
                .contains("could not read config")
        );
    }

    #[test]
    fn parses_and_validates_versioned_config() {
        let path = temp_file(
            "version = 1\n[device]\nvendor_id = 0x046d\nproduct_id = 0xc092\ninterface_number = 1\n[dpi]\nsensor = 0\nvalue = 1600\n[rate]\nhz = 500\n[rgb]\ncluster = 0\neffect_index = 1\ncolor = '#89b4fa'\n[wallust]\nenabled = true\npalette_file = '~/.cache/g102-control/palette.json'\ncolor_key = 'accent'\ncluster = 0\neffect_index = 1\n",
        );
        let config = AppConfig::load(Some(&path), false).unwrap();
        assert_eq!(config.device.product_id, Some(0xc092));
        assert_eq!(config.dpi.value, Some(1600));
        assert_eq!(config.rate.hz, Some(500));
        assert!(config.wallust.enabled);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_unknown_versions_invalid_values_and_unknown_keys() {
        let path = temp_file("version = 2\n");
        assert!(
            AppConfig::load(Some(&path), false)
                .unwrap_err()
                .contains("unsupported config version")
        );
        fs::write(&path, "version = 1\n[dpi]\nvalue = 0\n").unwrap();
        assert!(
            AppConfig::load(Some(&path), false)
                .unwrap_err()
                .contains("configured DPI")
        );
        fs::write(&path, "version = 1\nunknown = true\n").unwrap();
        assert!(
            AppConfig::load(Some(&path), false)
                .unwrap_err()
                .contains("unknown field")
        );
        fs::write(&path, "version = 1\n[device]\ninterface_number = 1\n").unwrap();
        assert!(
            AppConfig::load(Some(&path), false)
                .unwrap_err()
                .contains("interface_number` requires")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_only_rgb_triplets() {
        assert_eq!(validate_color("#89b4fa").unwrap(), [0x89, 0xb4, 0xfa]);
        assert!(validate_color("#89b4faff").is_err());
        assert!(validate_color("red").is_err());
    }

    #[test]
    fn rejects_conflicting_config_switches() {
        assert!(
            AppConfig::load(Some(Path::new("config.toml")), true)
                .unwrap_err()
                .contains("cannot be used together")
        );
    }
}
