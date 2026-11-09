use std::ffi::CString;
use std::time::Duration;

use hidapi::HidApi;

use crate::error::ClientError;
use crate::hid::{HidapiTransport, HidppDeviceInfo};
use crate::protocol::{
    FeatureDescriptor, FeatureDiscovery, FeatureId, HidppClient, ProtocolVersion, ReportKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInspection {
    pub device: HidppDeviceInfo,
    pub device_index: u8,
    pub report_kind: ReportKind,
    pub protocol_version: ProtocolVersion,
    pub feature_set_available: bool,
    pub features: Vec<FeatureDescriptor>,
}

#[derive(Debug)]
pub struct ProbeResult {
    pub device: HidppDeviceInfo,
    pub inspection: Result<DeviceInspection, String>,
}

/// A probed endpoint with its serialized HID++ client kept open for commands.
pub struct ConnectedDevice {
    pub inspection: DeviceInspection,
    pub client: HidppClient<HidapiTransport>,
}

/// Enumerates Logitech vendor HID++ collections and probes each endpoint
/// read-only. No model PID allowlist is used.
pub fn discover(api: &HidApi) -> Vec<ProbeResult> {
    HidapiTransport::enumerate(api)
        .into_iter()
        .filter(HidppDeviceInfo::is_candidate)
        .map(|device| {
            let inspection = inspect_one(api, device.clone()).map_err(|error| error.to_string());
            ProbeResult { device, inspection }
        })
        .collect()
}

pub fn connect(api: &HidApi, device: HidppDeviceInfo) -> Result<ConnectedDevice, ClientError> {
    let path = CString::new(device.path.as_bytes())
        .map_err(|_| ClientError::Transport("HID path contains an interior NUL byte".into()))?;
    let mut last_error = ClientError::Protocol(crate::error::ProtocolError::Timeout);

    for report_kind in [ReportKind::Long, ReportKind::Short] {
        let transport = HidapiTransport::open_path(api, &path).map_err(ClientError::Transport)?;
        let mut client =
            HidppClient::new(transport, 0xff, report_kind).with_timeout(Duration::from_millis(350));
        let protocol_version = match client.get_protocol_version() {
            Ok(version) => version,
            Err(error) => {
                last_error = error;
                continue;
            }
        };
        let FeatureDiscovery {
            feature_set_available,
            features,
        } = client.discover_features()?;
        return Ok(ConnectedDevice {
            inspection: DeviceInspection {
                device,
                device_index: 0xff,
                report_kind,
                protocol_version,
                feature_set_available,
                features,
            },
            client,
        });
    }
    Err(last_error)
}

fn inspect_one(api: &HidApi, device: HidppDeviceInfo) -> Result<DeviceInspection, ClientError> {
    connect(api, device).map(|connected| connected.inspection)
}

impl DeviceInspection {
    pub fn has_feature(&self, feature_id: u16) -> bool {
        self.features
            .iter()
            .any(|feature| feature.id == FeatureId(feature_id))
    }

    pub fn feature(&self, feature_id: u16) -> Option<&FeatureDescriptor> {
        self.features
            .iter()
            .find(|feature| feature.id == FeatureId(feature_id))
    }

    pub fn product_name(&self) -> &str {
        self.device.product.as_deref().unwrap_or("unknown product")
    }
}
