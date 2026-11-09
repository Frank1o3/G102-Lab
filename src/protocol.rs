use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::error::{ClientError, HidppDeviceError, ProtocolError};
use crate::hid::HidTransport;

pub const ROOT_FEATURE_INDEX: u8 = 0;
pub const ROOT_FEATURE_ID: FeatureId = FeatureId(0x0000);
pub const FEATURE_SET_ID: FeatureId = FeatureId(0x0001);
pub const SHORT_REPORT_ID: u8 = 0x10;
pub const LONG_REPORT_ID: u8 = 0x11;
pub const SHORT_REPORT_LEN: usize = 7;
pub const LONG_REPORT_LEN: usize = 20;
pub const SOFTWARE_ID: u8 = 0x0b;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId(pub u16);

impl std::fmt::Display for FeatureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:04x}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Short,
    Long,
}

impl ReportKind {
    pub fn id(self) -> u8 {
        match self {
            Self::Short => SHORT_REPORT_ID,
            Self::Long => LONG_REPORT_ID,
        }
    }

    pub fn report_len(self) -> usize {
        match self {
            Self::Short => SHORT_REPORT_LEN,
            Self::Long => LONG_REPORT_LEN,
        }
    }

    pub fn parameter_capacity(self) -> usize {
        self.report_len() - 4
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            SHORT_REPORT_ID => Some(Self::Short),
            LONG_REPORT_ID => Some(Self::Long),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub report_kind: ReportKind,
    pub device_index: u8,
    pub feature_index: u8,
    pub function_id: u8,
    pub software_id: u8,
    pub params: Vec<u8>,
}

impl Packet {
    pub fn new(
        report_kind: ReportKind,
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8,
        params: &[u8],
    ) -> Result<Self, ProtocolError> {
        if function_id > 0x0f {
            return Err(ProtocolError::InvalidFunction(function_id));
        }
        if software_id == 0 || software_id > 0x0f {
            return Err(ProtocolError::InvalidSoftwareId(software_id));
        }
        let maximum = report_kind.parameter_capacity();
        if params.len() > maximum {
            return Err(ProtocolError::ParameterTooLong {
                kind: report_kind.id(),
                actual: params.len(),
                maximum,
            });
        }
        Ok(Self {
            report_kind,
            device_index,
            feature_index,
            function_id,
            software_id,
            params: params.to_vec(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut report = vec![
            self.report_kind.id(),
            self.device_index,
            self.feature_index,
            (self.function_id << 4) | self.software_id,
        ];
        report.extend_from_slice(&self.params);
        report.resize(self.report_kind.report_len(), 0);
        report
    }

    pub fn decode(report: &[u8]) -> Result<Self, ProtocolError> {
        let report_id = *report
            .first()
            .ok_or(ProtocolError::MalformedPacket("empty report"))?;
        let report_kind =
            ReportKind::from_id(report_id).ok_or(ProtocolError::InvalidReportKind(report_id))?;
        if report.len() != report_kind.report_len() {
            return Err(ProtocolError::InvalidReportLength {
                kind: report_id,
                actual: report.len(),
                expected: report_kind.report_len(),
            });
        }
        let address = report[3];
        // Error replies repurpose byte 3 as the original feature index and
        // place the original function/software byte in parameter 0.
        let is_error = report[2] == 0xff;
        Ok(Self {
            report_kind,
            device_index: report[1],
            feature_index: report[2],
            function_id: if is_error { address } else { address >> 4 },
            software_id: if is_error { 0 } else { address & 0x0f },
            params: report[4..].to_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureDescriptor {
    pub id: FeatureId,
    pub index: u8,
    /// Raw protocol feature-type flags; interpretation varies by protocol version.
    pub feature_type: u8,
    pub version: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    pub major: u8,
    pub minor: u8,
}

pub struct HidppClient<T> {
    transport: T,
    pub device_index: u8,
    pub report_kind: ReportKind,
    timeout: Duration,
    next_software_id: u8,
    unsolicited: VecDeque<Packet>,
    unmatched: VecDeque<Packet>,
}

impl<T: HidTransport> HidppClient<T> {
    pub fn new(transport: T, device_index: u8, report_kind: ReportKind) -> Self {
        Self {
            transport,
            device_index,
            report_kind,
            timeout: Duration::from_millis(500),
            next_software_id: SOFTWARE_ID,
            unsolicited: VecDeque::new(),
            unmatched: VecDeque::new(),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn take_unsolicited(&mut self) -> Vec<Packet> {
        self.unsolicited.drain(..).collect()
    }

    pub fn take_unmatched(&mut self) -> Vec<Packet> {
        self.unmatched.drain(..).collect()
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    /// Returns a mutable reference to the underlying transport.
    ///
    /// Intended for test-only use to inject faults or inspect internal state
    /// without consuming the client.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn allocate_software_id(&mut self) -> u8 {
        let result = self.next_software_id;
        self.next_software_id = if result == 0x0f { 1 } else { result + 1 };
        result
    }

    /// Sends one serialized request and waits for its matching reply using the default client timeout.
    ///
    /// This method never retries. In particular, a timeout after a future
    /// mutating call would be ambiguous and must not be repeated automatically.
    pub fn request(
        &mut self,
        feature_index: u8,
        function_id: u8,
        params: &[u8],
    ) -> Result<Packet, ClientError> {
        self.request_with_timeout(feature_index, function_id, params, self.timeout)
    }

    /// Sends one serialized request and waits for its matching reply using a caller-specified timeout.
    ///
    /// Used for operations with distinct latency characteristics such as flash erase/write commits.
    /// Never retries automatically.
    pub fn request_with_timeout(
        &mut self,
        feature_index: u8,
        function_id: u8,
        params: &[u8],
        timeout: Duration,
    ) -> Result<Packet, ClientError> {
        let software_id = self.allocate_software_id();
        let request = Packet::new(
            self.report_kind,
            self.device_index,
            feature_index,
            function_id,
            software_id,
            params,
        )?;
        self.transport
            .write_report(&request.encode())
            .map_err(ClientError::Transport)?;
        let deadline = Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ProtocolError::Timeout.into());
            }
            let Some(report) = self
                .transport
                .read_report(remaining)
                .map_err(ClientError::Transport)?
            else {
                return Err(ProtocolError::Timeout.into());
            };
            // Some transport backends may return a report after their
            // requested timeout. Do not accept a matching acknowledgement that
            // arrived after this request's deadline (especially for writes).
            if Instant::now() > deadline {
                return Err(ProtocolError::Timeout.into());
            }
            let response = Packet::decode(&report)?;
            if response.device_index != self.device_index {
                self.unmatched.push_back(response);
                continue;
            }
            if is_matching_error(&response, &request)? {
                let code = response.params[1];
                return Err(ProtocolError::Device(HidppDeviceError { code }).into());
            }
            if response.feature_index == request.feature_index
                && response.function_id == request.function_id
                && response.software_id == request.software_id
                && response.report_kind == request.report_kind
            {
                return Ok(response);
            }
            if response.software_id == 0 {
                self.unsolicited.push_back(response);
            } else {
                self.unmatched.push_back(response);
            }
        }
    }

    pub fn get_protocol_version(&mut self) -> Result<ProtocolVersion, ClientError> {
        let response = self.request(ROOT_FEATURE_INDEX, 1, &[0, 0, 0x5a])?;
        require_params(ROOT_FEATURE_ID, 1, &response.params, 3)?;
        if response.params[2] != 0x5a {
            return Err(ProtocolError::MalformedPacket("Root ping data did not echo").into());
        }
        Ok(ProtocolVersion {
            major: response.params[0],
            minor: response.params[1],
        })
    }

    pub fn get_feature(
        &mut self,
        feature_id: FeatureId,
    ) -> Result<Option<FeatureDescriptor>, ClientError> {
        let [high, low] = feature_id.0.to_be_bytes();
        let response = match self.request(ROOT_FEATURE_INDEX, 0, &[high, low]) {
            Ok(response) => response,
            Err(ClientError::Protocol(ProtocolError::Device(error))) if error.code == 0x06 => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        require_params(ROOT_FEATURE_ID, 0, &response.params, 3)?;
        let index = response.params[0];
        if index == 0 && feature_id != ROOT_FEATURE_ID {
            return Ok(None);
        }
        Ok(Some(FeatureDescriptor {
            id: feature_id,
            index,
            feature_type: response.params[1],
            version: response.params[2],
        }))
    }

    /// Enumerates FeatureSet when present, resolving every feature ID again
    /// through Root so the packet index always comes from dynamic discovery.
    pub fn discover_features(&mut self) -> Result<FeatureDiscovery, ClientError> {
        // FeatureSet entries include a four-byte descriptor; short HID++
        // reports carry only three parameter bytes. Root lookups remain
        // available on those legacy endpoints.
        if self.report_kind == ReportKind::Short {
            return Ok(FeatureDiscovery {
                feature_set_available: false,
                features: self.discover_known_features()?,
            });
        }
        let Some(feature_set) = self.get_feature(FEATURE_SET_ID)? else {
            return Ok(FeatureDiscovery {
                feature_set_available: false,
                features: self.discover_known_features()?,
            });
        };
        let count_response = self.request(feature_set.index, 0, &[])?;
        require_params(FEATURE_SET_ID, 0, &count_response.params, 1)?;
        let count = count_response.params[0];
        if count > 128 {
            return Err(ProtocolError::FeatureSetTooLarge(count).into());
        }

        let mut features = vec![FeatureDescriptor {
            id: ROOT_FEATURE_ID,
            index: ROOT_FEATURE_INDEX,
            feature_type: 0,
            version: 0,
        }];
        features.push(feature_set);
        for table_index in 1..=count {
            let response = self.request(feature_set.index, 1, &[table_index])?;
            require_params(FEATURE_SET_ID, 1, &response.params, 4)?;
            let feature_id =
                FeatureId(u16::from_be_bytes([response.params[0], response.params[1]]));
            if feature_id == ROOT_FEATURE_ID || feature_id == FEATURE_SET_ID {
                continue;
            }
            let Some(feature) = self.get_feature(feature_id)? else {
                continue;
            };
            if feature.index != table_index {
                return Err(ProtocolError::FeatureSetIndexMismatch {
                    feature_id: feature_id.0,
                    table_index,
                    root_index: feature.index,
                }
                .into());
            }
            features.push(FeatureDescriptor {
                feature_type: response.params[2],
                version: response.params[3],
                ..feature
            });
        }
        Ok(FeatureDiscovery {
            feature_set_available: true,
            features,
        })
    }

    fn discover_known_features(&mut self) -> Result<Vec<FeatureDescriptor>, ClientError> {
        const KNOWN: &[u16] = &[
            0x0003, 0x0005, 0x2201, 0x2202, 0x8060, 0x8061, 0x8070, 0x8071, 0x8100, 0x8110, 0x1b04,
        ];
        let mut features = vec![FeatureDescriptor {
            id: ROOT_FEATURE_ID,
            index: ROOT_FEATURE_INDEX,
            feature_type: 0,
            version: 0,
        }];
        for id in KNOWN {
            if let Some(feature) = self.get_feature(FeatureId(*id))? {
                features.push(feature);
            }
        }
        Ok(features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct MockTransport {
        replies: VecDeque<Vec<u8>>,
        writes: Vec<Vec<u8>>,
    }

    impl MockTransport {
        fn new(replies: Vec<Vec<u8>>) -> Self {
            Self {
                replies: replies.into(),
                writes: Vec::new(),
            }
        }
    }

    impl HidTransport for MockTransport {
        fn write_report(&mut self, report: &[u8]) -> Result<(), String> {
            self.writes.push(report.to_vec());
            Ok(())
        }

        fn read_report(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, String> {
            Ok(self.replies.pop_front())
        }
    }

    struct DelayedReplyTransport {
        response: Vec<u8>,
        delay: Duration,
    }

    impl HidTransport for DelayedReplyTransport {
        fn write_report(&mut self, _report: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn read_report(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, String> {
            std::thread::sleep(self.delay);
            Ok(Some(self.response.clone()))
        }
    }

    fn reply(feature_index: u8, function: u8, software_id: u8, params: &[u8]) -> Vec<u8> {
        Packet::new(
            ReportKind::Long,
            0xff,
            feature_index,
            function,
            software_id,
            params,
        )
        .unwrap()
        .encode()
    }

    #[test]
    fn short_and_long_packets_encode_exact_report_sizes() {
        let short = Packet::new(ReportKind::Short, 0xff, 0, 1, 0x0b, &[0, 0, 0x5a]).unwrap();
        assert_eq!(short.encode(), [0x10, 0xff, 0, 0x1b, 0, 0, 0x5a]);

        let long = Packet::new(ReportKind::Long, 0xff, 0x13, 2, 1, &[0xaa]).unwrap();
        assert_eq!(long.encode().len(), LONG_REPORT_LEN);
        assert_eq!(&long.encode()[..5], &[0x11, 0xff, 0x13, 0x21, 0xaa]);
        assert!(long.encode()[5..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn packet_validation_rejects_bad_arguments_and_lengths() {
        assert!(matches!(
            Packet::new(ReportKind::Short, 0xff, 0, 16, 1, &[]),
            Err(ProtocolError::InvalidFunction(16))
        ));
        assert!(matches!(
            Packet::new(ReportKind::Long, 0xff, 0, 0, 0, &[]),
            Err(ProtocolError::InvalidSoftwareId(0))
        ));
        assert!(matches!(
            Packet::new(ReportKind::Short, 0xff, 0, 0, 1, &[0; 4]),
            Err(ProtocolError::ParameterTooLong { .. })
        ));
        assert!(matches!(
            Packet::decode(&[0x10, 0, 0, 0]),
            Err(ProtocolError::InvalidReportLength { .. })
        ));
    }

    #[test]
    fn decode_preserves_function_and_software_id() {
        let decoded = Packet::decode(&[0x10, 0xff, 0x12, 0x3b, 1, 2, 3]).unwrap();
        assert_eq!(decoded.feature_index, 0x12);
        assert_eq!(decoded.function_id, 3);
        assert_eq!(decoded.software_id, 0x0b);
        assert_eq!(decoded.params, [1, 2, 3]);
    }

    #[test]
    fn request_skips_unmatched_reply_and_returns_correlated_response() {
        let unrelated = Packet::new(ReportKind::Long, 0xff, 4, 2, 3, &[0x99])
            .unwrap()
            .encode();
        let matching = reply(0, 1, SOFTWARE_ID, &[4, 5, 0x5a]);
        let transport = MockTransport::new(vec![unrelated.clone(), matching]);
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        let response = client.request(0, 1, &[0, 0, 0x5a]).unwrap();
        assert_eq!(response.params[0..3], [4, 5, 0x5a]);
        assert_eq!(client.take_unmatched().len(), 1);
    }

    #[test]
    fn request_accepts_delayed_matching_reply_before_deadline() {
        let transport = DelayedReplyTransport {
            response: reply(0, 1, SOFTWARE_ID, &[4, 5, 0x5a]),
            delay: Duration::from_millis(10),
        };
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        let response = client
            .request_with_timeout(0, 1, &[0, 0, 0x5a], Duration::from_millis(200))
            .unwrap();
        assert_eq!(response.params[..3], [4, 5, 0x5a]);
    }

    #[test]
    fn request_rejects_matching_reply_returned_after_deadline() {
        let transport = DelayedReplyTransport {
            response: reply(0, 1, SOFTWARE_ID, &[4, 5, 0x5a]),
            delay: Duration::from_millis(150),
        };
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        let result = client.request_with_timeout(0, 1, &[0, 0, 0x5a], Duration::from_millis(100));
        assert!(matches!(
            result,
            Err(ClientError::Protocol(ProtocolError::Timeout))
        ));
    }

    #[test]
    fn request_parses_matched_hidpp_error() {
        // Error frame: report, index, 0xff, failed feature index,
        // failed function/software byte, error code, padding.
        let error = [
            0x11, 0xff, 0xff, 0, 0x1b, 0x09, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let transport = MockTransport::new(vec![error.to_vec()]);
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        let result = client.request(0, 1, &[]);
        assert!(matches!(
            result,
            Err(ClientError::Protocol(ProtocolError::Device(
                HidppDeviceError { code: 9 }
            )))
        ));
    }

    #[test]
    fn root_feature_set_discovery_resolves_indexes_dynamically() {
        let replies = vec![
            reply(0, 0, 0x0b, &[2, 0, 0]),          // Root -> FeatureSet index 2
            reply(2, 0, 0x0c, &[2]),                // FeatureSet count
            reply(2, 1, 0x0d, &[0, 1, 0, 0]),       // table entry 1: FeatureSet
            reply(2, 1, 0x0e, &[0x22, 0x01, 0, 1]), // table entry 2: DPI
            reply(0, 0, 0x0f, &[2, 0, 0]),          // Root -> DPI index 2
        ];
        let transport = MockTransport::new(replies);
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        let found = client.discover_features().unwrap();
        assert!(found.feature_set_available);
        assert_eq!(found.features.len(), 3);
        assert_eq!(found.features[2].id, FeatureId(0x2201));
        assert_eq!(found.features[2].index, 2);
        assert_eq!(found.features[2].version, 1);
        let transport = client.into_transport();
        assert_eq!(transport.writes.len(), 5);
        assert_eq!(transport.writes[0][2], 0); // Root index, not a guessed feature index.
    }

    #[test]
    fn root_ping_validates_echoed_data() {
        let transport = MockTransport::new(vec![reply(0, 1, SOFTWARE_ID, &[4, 5, 0x5a])]);
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        assert_eq!(
            client.get_protocol_version().unwrap(),
            ProtocolVersion { major: 4, minor: 5 }
        );
    }

    #[test]
    fn root_lookup_returns_runtime_feature_index() {
        let transport = MockTransport::new(vec![reply(0, 0, SOFTWARE_ID, &[7, 0, 2])]);
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        let feature = client.get_feature(FeatureId(0x2201)).unwrap().unwrap();
        assert_eq!(feature.index, 7);
        assert_eq!(feature.version, 2);
    }

    #[test]
    fn invalid_feature_index_error_means_feature_absent() {
        let error = [
            0x11, 0xff, 0xff, 0, 0x0b, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let transport = MockTransport::new(vec![error.to_vec()]);
        let mut client = HidppClient::new(transport, 0xff, ReportKind::Long);
        assert_eq!(client.get_feature(FeatureId(0x2201)).unwrap(), None);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDiscovery {
    pub feature_set_available: bool,
    pub features: Vec<FeatureDescriptor>,
}

fn require_params(
    feature: FeatureId,
    function: u8,
    params: &[u8],
    required: usize,
) -> Result<(), ProtocolError> {
    if params.len() < required {
        return Err(ProtocolError::MissingRequiredParameter {
            feature: feature.0,
            function,
            required,
            actual: params.len(),
        });
    }
    Ok(())
}

fn is_matching_error(response: &Packet, request: &Packet) -> Result<bool, ProtocolError> {
    if response.report_kind != request.report_kind || response.feature_index != 0xff {
        return Ok(false);
    }
    if response.params.len() < 2 {
        return Err(ProtocolError::MalformedPacket("short HID++ error response"));
    }
    Ok(response.function_id == request.feature_index
        && response.params[0] == ((request.function_id << 4) | request.software_id))
}
