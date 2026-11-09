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
5. Read 8071 descriptors, clusters, supported effects and software-control flags. The CLI setter claims software control using the G102 reference sequence and sends the effect with volatile-only flag `0x01`; it never changes onboard mode or NV configuration. This path is reference-derived and its successful visual result on the user's target still needs confirmation. Do not infer three independently addressable clusters; use the returned cluster count. Never use 0x8070 unless the endpoint actually advertises it and a separate codec is implemented.
6. Read 8100 profile descriptor, mode, current-profile response, directory and bounded sector contents. Current CLI operations use only getInfo/getMode/getCurrentProfile/memoryRead and do not switch mode or select profiles. Export preserves all writable sectors in a `G102OBK1` local container. The original Phase 4 implementation labeled directory and checksum interpretations as candidates pending a backup. Phase 5 validated the directory association and CRC against the supplied backup; see Phase 5 evidence below.
7. Record whether 1b04/8110 is advertised; do not set remapping. 0x8110 remains separate from profile storage.

Feature errors are evidence of unsupported function only when correctly matched; timeout does not prove absence.

## Hardware mutation gate

The 0x2201 and 0x8060 setter code is implemented and exercised only against scripted mock transports. It has not been validated on the target G102. Do not run setter commands against physical hardware until the exact model/firmware has passed the read-only checks above and the accepted values/mode ownership are recorded. Then test one reversible value, read it back, restore the original, and confirm restoration. The implementation never changes host/onboard mode, and no persistence claim is made.

These other physical mutations must remain unavailable until their specific evidence gates pass:

- RGB software-control takeover, individual colors and effect setters until exact target bytes and visual application are confirmed. The setter uses volatile-only persistence; flash/persistent writes remain unavailable. The CLI does not restore prior software ownership, and ownership cleanup remains a limitation.
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
   `cargo run -- onboard buttons g102-backup.obk` displays the 6 stored button slots at profile sector offset `0x20..0x38`. It notes that stored slots 1..6 correlate with default mouse buttons [Left, Right, Middle, Back, Forward, Cycle DPI], but physical wiring is unverified without hardware click tracing.
2. **Action support & safety gates**:
   Verified remappable targets are restricted to evidence-corroborated G102 format-4 actions: `left`, `right`, `middle`, `back`, `forward`, `dpi-cycle`, and `disable`. Unverified actions (keyboard modifiers/keys, consumer controls, macros, special codes other than 0x05) are rejected with descriptive errors to prevent profile corruption.
3. **Dry-run modification & byte diffing**:
   `cargo run -- onboard remap g102-backup.obk --slot 4 --action right --dry-run` displays the exact planned changes, old/new checksums, and exact byte diffs (e.g. byte 0x2f changing 0x08 -> 0x02, and CRC at 253..255 changing 0xb3d1 -> 0xdd49). It confirms that all other 252 bytes in sector 1 and all other 15 sectors are untouched.
4. **Offline file writer safety**:
   `cargo run -- onboard remap g102-backup.obk out.obk --slot 4 --action right` writes only to a distinct output file and refuses to overwrite the input backup file.
5. **Byte-level comparison**:
   `cargo run -- onboard diff g102-backup.obk out.obk` verifies that sector 1 differences are strictly confined to the button table and sector CRC.
6. **No hardware writes**:
   All Phase 6 commands are offline only; no HID++ write packets or memory write transactions are executed.

## Phase 7 safe onboard profile writing evidence

Phase 7 implements physical onboard memory writing with defense-in-depth safety checks and verified mock transaction coverage:

1. **HID++ 0x8100 write protocol verification**:
   - `write_sector_with_tracking` sends `FN_START_WRITE = 6` with `[sector_h, sector_l, 0, 0, len_h, len_l]`, 16 chunks via `FN_WRITE_DATA = 7` (16 bytes each, zero-padded for the final 15-byte tail), and commits via `FN_END_WRITE = 8`.
   - `test_write_sector_protocol_packets` verifies all 18 packets against exact expected byte payloads.
2. **Failure injection testing**:
   - `test_write_failure_injection_start_write`: Proves transaction aborts with `StartWriteFailed` if device rejects write window.
   - `test_write_failure_injection_chunk`: Injects device error at 48th byte offset; verifies `WriteChunkFailed { offset: 48, .. }` is returned and commit `FN_END_WRITE` is NOT sent.
   - `test_write_failure_injection_commit`: Verifies error reporting if flash commit fails.
3. **Mandatory confirmation & dry-run gating**:
   - `test_execute_remap_refuses_without_confirm_write` proves that calling write functions without `confirm_write: true` returns `SafetyRefusal` and sends zero HID reports.
   - CLI commands `onboard set-button` and `onboard restore` enforce `--dry-run` or `--confirm-write` upfront before any HID communication.
4. **Pre-write automatic backup & on-disk verification**:
   - Before any flash transaction starts, a fresh raw backup is exported to disk (`g102_backup_<timestamp>.obk` or user-specified `--backup-file`).
   - The backup is re-read from disk and parsed to verify integrity before any write packets are sent.
   - `save_backup_file` and unique path generators refuse to overwrite existing files.
5. **Readback byte-for-byte, checksum, and profile decode verification**:
   - `test_execute_remap_successful_with_mock`: Full end-to-end simulated device write; verifies backup export, 18 write packets, readback of 255 bytes, byte-for-byte comparison, CRC16-CCITT check, and format-4 profile decode verification.
   - `test_execute_remap_detects_readback_byte_mismatch`: Corrupts a button byte during readback and proves `ReadBackMismatch { offset: 0x2d, .. }` aborts the operation.
   - `test_execute_remap_detects_readback_checksum_mismatch`: Corrupts checksum bytes during readback and proves `ReadBackMismatch { offset: 253, .. }` aborts the operation.
   - `test_execute_restore_with_mock`: Full end-to-end restore from backup file, including safety backup export, sector write, readback verification, and profile restore.

