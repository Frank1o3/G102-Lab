# Validation strategy

Validation separates deterministic software tests from tests requiring the actual G102. No hardware write has occurred in this research phase. Default automated tests must never require a device.

## Software-only tests

- Packet encode/decode for 7-byte 0x10 and 20-byte 0x11; parameter bounds, zero padding, endian, malformed/truncated input and unsupported report IDs.
- Response correlation by device index, feature index, function/software ID; error packets; unrelated replies; unsolicited notifications; timeout and write-outcome-unknown behavior.
- Root GetFeature and protocol version; absent/hidden feature; version/index cache invalidation on reconnect.
- Scripted mock transport tests for request serialization, concurrent callers, notifications, close, hotplug and multiple endpoints.
- DPI codecs: sensor count, list, current/default, endian, unsupported sensor/value, min/max/step boundary.
- 8060 bitmap periods and Hz mapping, empty list and invalid values. 8061 separate codec fixtures.
- RGB discovery for device/cluster/effect query responses, effect ID versus slot mapping, malformed counts, persistence capability parsing, and only validated payload layouts.
- Onboard descriptor/directory bounds, unknown IDs, sector read chunking including overlap tail, format rejection, lossless preservation of unknown bytes.
- CRC known vectors and captured-sector validation for each explicitly supported format. No generic assumption that CRC-16 applies everywhere.
- Button action fixtures for each known 4-byte action plus unknown byte preservation; G-Shift/macro only if supported codec exists.
- Writer simulations with failure at pre-read, backup, start, each data chunk, end, readback and checksum; assert no blind retries and backup remains available.
- Palette JSON valid/malformed/schema version/RGB parsing, duplicate hash, debounce bursts, atomic rename, partial write, file changed mid-read, missing file, last-good retention and manual override precedence.
- Reconnection and two-device mock tests ensure no stale feature indexes or response cross-talk.
- Configuration version/migration/range tests and logging privacy checks.

## Actual G102 read-only validation

Record date, physical model label, exact VID/PID, product string, firmware, direct USB versus receiver, Linux kernel/hidapi backend, endpoint usage/interface and report descriptor. Save raw request/reply bytes without serial by default.

1. Enumerate candidate endpoints and probe Root ping/version only.
2. Discover feature IDs, indexes, versions. Repeat and compare.
3. Read DPI sensor count/list/current/default. Exercise physical DPI button while observing values only if this does not require writes.
4. Read 8060/8061 presence and supported/current values. Read 8100 mode only; do not switch it.
5. Read 8071 descriptors, clusters, supported effects and software-control flags. The CLI setter sends the G102-reference software-control sequence and setter parameter `0x01`; the reference describes the parameter as volatile, but target display/effect persistence has not been established. `rgb info` capability fields are not evidence that a particular setting persisted. This path is reference-derived and its visible result on the target still needs confirmation. The command does not issue 0x8100 onboard-profile writes. Do not infer three independently addressable clusters; use the returned cluster count. Unknown effect IDs remain read-only. Never use 0x8070 unless the endpoint advertises it and a separate codec is implemented.
6. Read 8100 profile descriptor, mode, current-profile response, directory and bounded sector contents. Current CLI operations use only getInfo/getMode/getCurrentProfile/memoryRead and do not switch mode or select profiles. Export preserves all writable sectors in a `G102OBK1` local container. The original Phase 4 implementation labeled directory and checksum interpretations as candidates pending a backup. Phase 5 validated the directory association and CRC against the supplied backup; see Phase 5 evidence below.
7. Record whether 1b04/8110 is advertised; do not set remapping. 0x8110 remains separate from profile storage.

Feature errors are evidence of unsupported function only when correctly matched; timeout does not prove absence.

## Hardware mutation gate

The 0x2201 and 0x8060 setter code is implemented and exercised only against scripted mock transports. It has not been validated on the target G102. Do not run setter commands against physical hardware until the exact model/firmware has passed the read-only checks above and the accepted values/mode ownership are recorded. Then test one reversible value, read it back, restore the original, and confirm restoration. The implementation never changes host/onboard mode, and no persistence claim is made.

These other physical mutations must remain unavailable until their specific evidence gates pass:

- RGB software-control takeover, individual colors and effect setters until exact target bytes and visual application are confirmed. The setter's parameter `0x01` has a reference-described volatile interpretation, but actual display/effect persistence is unverified. RGB effects and 0x8100 onboard-flash profiles are separate; the CLI does not write onboard profiles. The CLI does not restore prior software ownership, and ownership cleanup remains a limitation.
- All 8100 sector writes, profile activation, button remap, G-Shift, macros and checksum generation until exact G102 format is documented and tested.
- 1b04 or 8110 setters until semantics and persistence scope are known.

After each mutation is independently enabled, test one reversible setting, read it back, restore original, verify restoration, then test reconnect and power cycle. Persistent remapping acceptance additionally requires shutdown, unplug, reboot and another host to confirm assignment resides in mouse. Never use an unknown format or active profile as a test target.

## Profile backup/restore gate

Before future writer validation: capture complete raw sectors and directory; record hashes; validate format-specific checksum; preserve backup outside mutable config. Perform edits on an offline copy and diff. Require format-specific write protocol and recoverable restore. Read back exact bytes and checksum after write. Test restoration using a known supported model first; G102 writes require G102-specific approval evidence. If any field/layout/checksum is uncertain, writer remains disabled.

Phase 4 local hardware attempt was read-only and failed because `/dev/hidraw3` was absent in the implementation environment. The user's report that 0x8100 is at runtime index 0x0f is not a descriptor or format capture. The supplied raw backup now establishes descriptor, sector geometry, action records and CRC for this capture; physical button mapping/action behavior and write feasibility remain unresolved.

## Completion criteria

Milestone passes only when software tests pass and hardware observations satisfy the milestone's acceptance criteria in PLAN.md. A README support entry or successful device enumeration alone is not proof. Update RESEARCH.md, unresolved questions, capability scope and test evidence whenever a hardware claim changes.

## Phase 5 offline profile decoder evidence

`g102-backup.obk` is a 4203-byte `G102OBK1` v1 container: 27-byte header plus sixteen records (6-byte record header + 255-byte body). The descriptor identifies memory model 1, format 4, macro format 1, six buttons, and sixteen writable sectors. The active slot is 1 and its directory entry maps to sector 1. CRC-16/CCITT-FALSE matches programmed sectors 0 and 1 with stored values 0x587c and 0xb3d1. This validates only the supplied backup. The offline decoder interprets fields according to the recovered base-v1 schema, corroborated by these bytes; it does not establish physical button order or action behavior. Unknown bytes are retained. No onboard write or mode change is performed.

Run `cargo run -- onboard decode g102-backup.obk` to inspect it.

## Phase 6 offline button remapping preparation evidence

Button remapping preparation implements structured action representations, button inspection, offline profile editing, dry-run diffing, and profile integrity validation without hardware communication:

1. **Button table inspection**:
   `cargo run -- onboard buttons g102-backup.obk` displays the six stored four-byte encodings at profile sector offset `0x20..0x38`. Familiar action names are reference interpretations of those bytes; neither their behavior nor their mapping to physical buttons is verified without behavioral hardware tracing.
2. **Action support & safety gates**:
   The offline editor accepts only the existing encodings `left`, `right`, `middle`, `back`, `forward`, `dpi-cycle`, and `disable`, based on the supplied backup and reference schema. This does not claim their physical behavior or button positions are verified. Keyboard modifiers/keys, consumer controls, macros, and other special codes remain rejected.
3. **Dry-run modification & byte diffing**:
   `cargo run -- onboard remap g102-backup.obk --slot 4 --action right --dry-run` displays the exact planned changes, old/new checksums, and exact byte diffs (e.g. byte 0x2f changing 0x08 -> 0x02, and CRC at 253..255 changing 0xb3d1 -> 0xdd49). It confirms that all other 252 bytes in sector 1 and all other 15 sectors are untouched.
4. **Offline file writer safety**:
   `cargo run -- onboard remap g102-backup.obk out.obk --slot 4 --action right` writes only to a distinct output file and refuses to overwrite the input backup file.
5. **Byte-level comparison**:
   `cargo run -- onboard diff g102-backup.obk out.obk` verifies that sector 1 differences are strictly confined to the button table and sector CRC.
6. **No hardware writes**:
   All Phase 6 commands are offline only; no HID++ write packets or memory write transactions are executed.

## Phase 7 scripted onboard transaction coverage (not hardware evidence)

Phase 7 added a mockable 0x8100 transaction implementation. Its function IDs and simulated packets do not verify G102 flash-write behavior. No physical write was performed. In particular, the supplied 255-byte backup establishes sector length only; it does not establish how the device handles a 16-byte transfer containing the final 15 sector bytes plus a 256th byte. The previous “zero-padded” description was not evidence and is superseded by Phase 8's fail-closed policy.

Scripted tests cover transaction packet construction, injected start/data/commit failures, backup serialization, sector-1 isolation, checksum validation, readback mismatch detection, and simulated restoration. These establish host behavior only. The simulator cannot prove firmware acceptance, flash effects, or restoration behavior on a G102.

## Phase 8 flash-write protocol hardening (software-only)

- Non-aligned writes, including the 255-byte G102 sector, are refused before `FN_START_WRITE` unless a caller explicitly supplies the final transfer byte. There is no default zero or `0xff`; an explicit value remains hardware-unverified. Aligned transfers need no tail-byte policy.
- `FN_END_WRITE` has an operation-specific configurable acknowledgement timeout. The 2,000 ms default is a conservative software policy, not a measured G102 latency. CLI `--commit-timeout-ms` is retained for the API but cannot enable physical profile writes while the gate is closed.
- Any timeout, disconnect, malformed response, or device error after attempting `FN_END_WRITE` is `PotentiallyCommitted` unless readback determines status. No retry or second write follows an ambiguous outcome. A delayed response returned after the request deadline is rejected by the client.
- The state model distinguishes `Uncommitted`, `PotentiallyCommitted`, `Committed` (acknowledged, readback unverified), and `Verified`. Recovery instructions use read-only `onboard export` and `onboard diff`, and explicitly prohibit restore or another write while status is uncertain.
- Public profile write APIs and CLI `onboard set-button` / `onboard restore` are disabled pending exact model/firmware validation; CLI refusal occurs before HID initialization. Dry-runs and read-only operations remain available. No key-remapping feature was added.

Still hardware-dependent and unresolved: whether the final 256th transfer byte is ignored, consumed, or must have a particular value; worst-case `FN_END_WRITE` latency; whether a matched device error guarantees no commit; whether disconnect clears volatile staging; and which exact G102 USB PID, firmware, and connection mode share the observed read-only profile format. Do not infer these from the backup, mock, references, or marketing name. Resolve via recorded device-specific evidence before considering any physical mutation.

## Phase 9 final refinement (mock/offline only)

The user reports that diagnostics passed on `046d:c092` and that an offline remap changed four bytes in one button record and two CRC bytes while preserving unrelated bytes. This is a user report, not a locally captured session; firmware and connection metadata were not supplied. It supports the offline container transformation only and does not verify physical action behavior or slot-to-button mapping.

RGB reporting distinguishes software-control state, raw/device-reported persistence capability fields, the setter's reference-described parameter `0x01`, and observed persistence. Capability flags and a setter acknowledgement do not prove an effect persisted. The RGB command path does not issue 0x8100 onboard-profile writes. Unknown effect IDs remain visible as unknown and are not set.

Button table status labels refer to encoding evidence, not behavioral verification. Physical button positions and action behavior remain unverified. The offline editor continues to reject keyboard, consumer, macro, and unsupported special-action encodings. Mock/offline tests cover non-empty restore previews, modified-profile restoration and CRC, malformed/checksum-invalid backups, unsupported restore sectors, original-backup preservation, and byte preservation outside the requested action/CRC.

Physical `onboard set-button` and `onboard restore` remain disabled in both the CLI and public APIs; `--confirm-write` cannot override the gate. No physical writes were performed for Phase 9. There is no Piper-like remapping expansion, GUI, daemon, or unknown-effect setter support.

## Phase 10 release/system integration checks (2026-11-09)

Software-only checks added:

- Rust config tests cover defaults, explicit missing files, version/range/unknown-key validation, RGB triplet parsing, and conflicting config flags. CLI tests cover global config options, numeric CLI override over config defaults, preferred device identity, explicit `--device` override, and the onboard write refusal.
- Python standard-library tests use temporary JSON/TOML files and a fake CLI to verify valid palette invocation, malformed/missing/invalid colors, refusal of non-static/unreported effects, enabled config defaults, and disabled integration behavior.
- `makepkg --cleanbuild --nodeps` built the release package and ran its Rust/Python tests on CachyOS. The archive was inspected: binaries/helper are mode 0755; docs, sample config, template, license, and udev rule are mode 0644. `ldd` shows libudev as the required non-glibc runtime library (`systemd-libs`).
- The release binary was invoked from `/tmp`; `--version` worked and invalid command/config inputs returned nonzero before HID access.

System/hardware checks still pending:

- Pacman install/removal was not run because `sudo -n` requires a password in this environment. The recipe and package archive were built/audited, but package-manager lifecycle is not claimed as tested.
- The rule is scoped to the observed `046d:c092` interface 01 endpoint. It was not installed or reloaded, and no ACL was granted. A regular-user probe before the rule was present saw `Permission denied`; successful non-root access remains unverified.
- Wallust template syntax was checked against official documentation and adapter behavior against fixtures, but the Wallust executable/hook ordering was not run here.
- No physical DPI, polling, RGB, or onboard-write operation was run. All physical setting behavior and RGB visibility/persistence remain unverified. The 0x8100 physical writer remains disabled; `--confirm-write` cannot bypass the gate.
