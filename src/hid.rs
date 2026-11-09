use std::collections::HashSet;
use std::ffi::CStr;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

pub const LOGITECH_VENDOR_ID: u16 = 0x046d;
pub const HIDPP_USAGE_PAGE: u16 = 0xff00;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidppDeviceInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

impl HidppDeviceInfo {
    pub fn is_candidate(&self) -> bool {
        self.vendor_id == LOGITECH_VENDOR_ID && self.usage_page == HIDPP_USAGE_PAGE
    }
}

pub trait HidTransport {
    fn write_report(&mut self, report: &[u8]) -> Result<(), String>;
    /// Returns one complete HID report, or None when the timeout expires.
    fn read_report(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, String>;
}

pub struct HidapiTransport {
    device: HidDevice,
}

impl HidapiTransport {
    pub fn open_path(api: &HidApi, path: &CStr) -> Result<Self, String> {
        api.open_path(path)
            .map(|device| Self { device })
            .map_err(|error| error.to_string())
    }

    pub fn enumerate(api: &HidApi) -> Vec<HidppDeviceInfo> {
        let mut seen_paths = HashSet::new();
        api.device_list()
            .filter(|info| {
                info.vendor_id() == LOGITECH_VENDOR_ID && info.usage_page() == HIDPP_USAGE_PAGE
            })
            .map(|info| HidppDeviceInfo {
                path: info.path().to_string_lossy().into_owned(),
                vendor_id: info.vendor_id(),
                product_id: info.product_id(),
                usage_page: info.usage_page(),
                usage: info.usage(),
                interface_number: info.interface_number(),
                manufacturer: info.manufacturer_string().map(str::to_owned),
                product: info.product_string().map(str::to_owned),
            })
            .filter(|info| seen_paths.insert(info.path.clone()))
            .collect()
    }
}

impl HidTransport for HidapiTransport {
    fn write_report(&mut self, report: &[u8]) -> Result<(), String> {
        self.device
            .write(report)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn read_report(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, String> {
        let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let mut buffer = [0u8; 64];
        self.device
            .read_timeout(&mut buffer, timeout_ms)
            .map(|length| (length != 0).then(|| buffer[..length].to_vec()))
            .map_err(|error| error.to_string())
    }
}
