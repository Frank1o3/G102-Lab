# Implementation specification

This describes future work; it is not application code. Start with a library and mock transport. Hardware-specific unknowns remain explicit and must not be guessed.

## Suggested crate/module layout

```text
src/
  lib.rs
  error.rs
  transport/{mod.rs, hidapi.rs or hidraw.rs, discovery.rs}
  protocol/{packet.rs, dispatcher.rs, root.rs}
  device/{manager.rs, endpoint.rs, identity.rs, capabilities.rs}
  features/{mod.rs, dpi_2201.rs, dpi_2202.rs, rate_8060.rs,
            rate_8061.rs, rgb_8070.rs, rgb_8071.rs,
            onboard.rs, reprogrammable.rs, button_spy.rs}
  profiles/{mod.rs, raw.rs, codec.rs, format_registry.rs, backup.rs, writer.rs}
  palette/{mod.rs, schema.rs, file_provider.rs, mapping.rs}
  config.rs
  coordinator.rs
  bin/g102-control.rs
```

Split crates only when library boundaries stabilize. No GUI framework in the initial implementation.

## Core types and interfaces

Use distinct newtypes: FeatureId(u16), FeatureIndex(u8), DeviceIndex(u8), FunctionId(u8), SoftwareId(u8), ReportKind, EndpointId and ConnectionGeneration. A packet contains report kind, target index, feature index, function nibble, software ID and bounded parameter bytes. Encode exactly 7 or 20 bytes for report 10/11; any additional report kind requires separately validated support. Decode short reads, HID error replies and normal replies without truncating.

Transport contract: enumerate candidate metadata, open by path, read report with deadline, write one complete report, expose endpoint identity and close. Fake transport implements same contract for scripted tests. Linux backend checks HID vendor collection/report descriptors and permissions. Backend must document report-ID handling.

Protocol client contract: execute one request and return matching response or typed error; route unsolicited events to a queue; do not discard nonmatching messages. Queueing/locking is per physical endpoint. Software IDs must be valid nonzero nibble values according to policy and echoed replies are checked. A timeout after mutating request returns OutcomeUnknown, not retry.

Root discovery API returns protocol version and FeatureDescriptor { id, index, version, flags }. Root is index zero. Cache only within a connection generation; clear on reconnect. Feature absence is a capability, not a startup failure.

## Capability representation

DeviceCapabilities includes observed feature descriptors plus validated domain data:

- DPI sensors with supported list/range/step, current/default when known.
- report-rate choices and active codec 8060 or 8061.
- lighting feature generation and enumerated zones, location IDs, effects (protocol effect ID distinct from device slot), persistence/ownership capability.
- onboard descriptor IDs/geometry and an optional registered profile codec; unsupported if absent.
- remapping methods separately: live 1b04, host 8110, onboard profile button tables.

Store evidence/source and validation scope with codecs (model/revision/firmware). Do not infer capability from PID table.

## Feature contracts

2201 DPI service: count/list/get/set sensor. Convert wire units with checked endian parsing. Set rejects values outside discovered support and reads back where possible. 2202 is independent codec; no shared fallback unless its own discovery and response schema validates.

8060 service: parse bitmap/current milliseconds; present Hz derived from period. 8061 remains separate index/capability codec. A change does not implicitly switch 8100 mode. Mode change is a distinct explicit operation because it can change device ownership and persist.

RGB zone model: LightingZone { device_zone_id, location, effects, capabilities }; effect slots are enumerated and not hardcoded. StaticColor uses RGB8; effect-specific parameter struct only after exact layout is validated. Service distinguishes software-control requests, reported persistence capabilities, and observed persistence outcomes. A setter acknowledgement or capability flag is not proof of persistence. Capture existing owner, acquire only via validated sequence, release safely. If hardware doesn't expose a reliable release, don't claim support.

Onboard command service: get descriptor, get/set mode, enumerate profiles, get/set active profile, read memory. Model raw sector as bytes plus sector ID and descriptor metadata. Do not parse until codec is selected by recognized format/model and tested scope. ProfileFormatCodec has parse, validate, edit-copy and checksum methods. Unknown bytes are retained. Format IDs seen in other mice are not registered as G102 until evidence is added.

ButtonAction should represent default/unassigned, disabled, mouse button, keyboard usage+modifier, consumer usage, special function, macro reference, and Unknown(raw bytes). Preserve exact bytes for unknown actions. Separate semantic action from wire encoding. G-Shift and macro storage are unavailable unless target layout and trigger behavior are confirmed.

## Persistent writer contract

Writer accepts explicit WritePlan only when format codec returns supported and checksum-valid. Plan contains original sector snapshot/digest, modified full sector, decoded diff, target profile, format/evidence scope and checksum. Re-read target before commit and abort on difference. Save original raw sector and metadata to backup before any write. Preserve all uninterpreted bytes. Validate bounds, descriptor geometry, original and new checksum. Then perform the format's exact start/data/end transaction; no generic writer guesses chunk protocol. Read full sector back, compare byte-for-byte, validate checksum and reparse. Never blind retry after uncertain write; expose recovery instructions and backup restoration. No format writes enabled until G102-specific validation is documented.

Phase 8 hardening status: the existing 255-byte sector transaction has no default final transfer byte; non-aligned data fails preflight unless a byte is explicitly provided, and that byte is not declared G102-compatible. `FN_END_WRITE` uses a configurable 2,000 ms policy default; every error after attempting commit is potentially committed unless readback resolves it. The public profile-write APIs and CLI are hard-disabled until exact target validation. Phase 8 adds no remapping feature; offline profile editing remains separate and non-device-mutating.

## Palette schema/configuration

**Phase 10 implementation:** `src/config.rs` provides optional version-1 TOML and performs validation without HID access or automatic writes. `scripts/g102-control-wallust` reads the `org.g102-control.palette/v1` JSON produced by the Wallust template in `integrations/wallust/`; it validates one semantic color and checks the requested slot against `rgb effects` output before invoking `g102-control rgb set`. `source` is emitted by the template; `generated_at` is optional and is not required by the adapter. Palette paths are user-configured because Wallust has no project-specific default path. No watcher, last-good cache, debounce, file monitoring, or automatic application is implemented.

The implemented config has preferred USB VID/PID/interface selection, optional DPI/rate/RGB defaults, and opt-in Wallust settings. Explicit CLI values override config; config values only fill omitted values for an explicitly invoked command. Do not store raw profile data in user config. A generic provider, zone mapping coordinator, watcher, and manual override system remain future work.

## Coordinator/interface

Coordinator owns desired state, current confirmed state and per-device feature clients. On palette update, map to currently enumerated lighting zones, diff against confirmed colors and send only changes. If device is absent, keep desired value and apply after rediscovery only if user enabled auto-apply; do not change ownership implicitly. Device detach invalidates in-flight requests, feature indexes and caches.

Start with a diagnostic/read CLI: list devices, inspect features, read DPI/rate/RGB/profile descriptor, export raw profile backup. Mutating subcommands should be gated and explicit. Persistent write UX is not to be built before format verification. A later daemon or GUI calls library/coordinator APIs, never transport directly.
