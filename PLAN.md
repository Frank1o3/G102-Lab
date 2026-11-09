# Implementation roadmap

Each milestone is gated on evidence and acceptance criteria. DPI and polling setter code now exists, but physical use remains gated on exact-model read-only validation. Onboard-memory mutations remain unavailable until the later safety review and format verification.

## Current implementation status (2026-11-09)

Phases 1 and 2 are implemented in software: packet codec, mockable serialized client, Linux hidapi enumeration, Root/FeatureSet discovery, read-only inspection, and 0x2201 DPI / 0x8060 polling read/set services and CLI. Mock tests validate parsing, dynamic feature indexes, supported-value checks, setters and read-back. No device settings were changed during development, and no connected exact-revision G102 has been verified. Setters never change host/onboard mode and make no persistence claim.

Phase 3 RGB inspection and setters for device-reported Off, Static, Cycle, Wave, and Breathing effects are implemented with mocked protocol coverage. The user supplied a target capability dump and reported an acknowledged static setter that did not visibly light the mouse. The implementation sends the reference-described software-control sequence and setter parameter `0x01`; actual RGB persistence remains unverified. Device-reported persistence capability fields are not persistence results, and the RGB command path issues no 0x8100 onboard-profile writes.

Phase 4 read-only 0x8100 inspection and raw backup export are implemented. Phase 5 adds strict offline `G102OBK1` parsing and an evidence-scoped format-4 decoder using `g102-backup.obk`. Descriptor, 16x255 writable geometry, six actions, and CRC match on the supplied backup; no mouse writes or mode/profile changes are implemented.

Phase 6 implements offline button-remap preparation: structured action representations, button inspection CLI (`onboard buttons FILE`), an offline profile editor (`onboard remap INPUT [OUTPUT] --slot N --action TARGET [--dry-run]`), and byte-level diffing (`onboard diff FILE_A FILE_B`). Unverified actions (keyboard, consumer, macro, unverified special opcodes) are rejected; unrelated sector fields and sectors remain byte-for-byte identical. This path edits local backup files only and is not expanded by Phase 8.

Phase 7 added a scripted 0x8100 write-transaction implementation with backup, sector-1 isolation, checksum, and readback safeguards. Its transaction shape and simulated success are not G102 hardware validation. The 255th-byte treatment and commit behavior were not established; an earlier test's padding convention was only a host-side assertion. The `--confirm-write` flag previously exposed physical mutation despite the documented evidence gate. Phase 8 closes the physical path in both the CLI and public library APIs. No hardware writes were performed.

## Phase 8 — Flash-write protocol hardening (complete; software-only)

This hardening task does not add Piper-like key remapping or extend offline remapping. Existing offline backup editing remains offline-only. It addresses the Phase 7 write transaction solely to make its uncertainty fail closed:

- A non-16-byte-aligned write (including a 255-byte sector) is refused before `FN_START_WRITE` unless a tail byte is explicitly supplied. No default byte is selected; an explicit value remains unverified for G102.
- `FN_END_WRITE` uses the configurable `WriteOptions::commit_timeout` (2,000 ms policy default). This is not a measured G102 worst-case latency. Every error after the commit request is attempted is `PotentiallyCommitted` unless readback establishes otherwise; no automatic retry or subsequent write is allowed.
- Transaction states distinguish `Uncommitted`, `PotentiallyCommitted`, `Committed` (acknowledged, not yet verified), and `Verified` (readback checks passed). Recovery guidance is read-only first (`onboard export`, then `onboard diff`) and prohibits restore/write while commit status is unknown.
- Transport clients reject a response returned after the request deadline. Scripted tests cover delayed and late responses, timeout, disconnection, lost commit acknowledgement, and no retry; they do not establish device behavior.
- Public profile write APIs and CLI `onboard set-button`/`restore` are disabled until exact G102 model/firmware validation is documented. Dry-run, export, and offline operations remain available.

Hardware-dependent uncertainties remain: whether the 256th transferred byte is ignored/consumed or has a required value; worst-case `FN_END_WRITE` latency; whether a matched device error guarantees no commit; whether disconnect clears volatile staging; and which exact G102 PID/firmware/connection modes share these behaviors. Resolving these requires read-only evidence and separately approved hardware validation. No physical writes were performed.

## Phase 9 — Final refinement and reliability (complete; software-only)

RGB reporting distinguishes software-control requests, device-reported persistence capability fields, reference-described setter parameters, actual display/effect persistence, and separate 0x8100 onboard flash. Button labels identify corroborated encodings only; action behavior and physical slot mapping remain unverified. Unknown effect IDs remain read-only, and unsupported action encodings remain rejected.

Mock/offline tests cover non-empty restore previews, modified-profile restoration and CRC validation, malformed/checksum-invalid backups, unsupported restore sectors, original-backup preservation, and unrelated-byte/sector isolation. The CLI help and diagnostics were reviewed against actual command behavior. Physical write gates remain unchanged. No daemon, GUI, frontend, new remapping capability, or Phase 10 work was added.

## Milestone 0 — Documentation and evidence baseline

Tasks: preserve reference clones; record exact clone revisions; establish research evidence labels and device test log format.

Acceptance: root docs agree; reference paths resolve; no application source is mistaken for reference code.

Tests: documentation link/path check and consistency review.

Completion: project constraints and unresolved hardware questions recorded.

## Milestone 1 — Read-only Linux discovery

Tasks: create Rust library/project only in a later implementation task; Linux HID endpoint enumeration, identity and permissions diagnostics; read-only Root ping and feature discovery; serialize each endpoint.

Dependencies: choose hidapi vs direct hidraw behind trait.

Outcome: inventory of connected Logitech HID++ endpoints without settings writes.

Acceptance: reports VID/PID/interface/usage, protocol version, feature ID/index/version; rejects non-HID++ endpoints; supports multiple devices; reconnect rediscovery.

Tests: packet/discovery mocks, malformed replies, timeout, notifications, no device permission case. Actual G102 required to validate endpoint and feature table.

## Milestone 2 — Protocol core and mock suite

Tasks: short/long encoding/decoding, response correlation, error decoding, event dispatch, timeout semantics, safe retry policy.

Outcome: independently testable transport core.

Acceptance: no feature/business logic in transport; no concurrent reader; unmatched packets do not corrupt calls; write timeout is outcome-unknown.

Tests: byte-exact fixtures and scripted mock endpoint.

## Milestone 3 — Read-only G102 capability reports

Tasks: query 2201 sensor/range/current/default, 8060 rates/current, 8070/8071 presence/info, 8100 descriptor/mode/directory/profile dumps only with safe bounded reads; 1b04/8110 presence only.

Dependency: actual G102 attached; capture PID, firmware, all raw replies.

Outcome: hardware evidence record resolving target feature support.

Acceptance: repeated read-only queries stable; all values annotated with evidence; unknown bytes preserved.

Tests: mock parsing plus actual G102 repeatability; no setters.

## Milestone 4 — Volatile DPI and polling

Tasks: implement typed DPI and rate read/write services; boundaries and supported values; make host/onboard transitions explicit.

Dependency: milestone 3 validates payloads and mode effects.

Outcome: user can alter current runtime sensor DPI/rate without flash modification.

Acceptance: setting accepted value and readback match; unsupported values rejected before transport; no silent host-mode change.

Tests: unit/mock; controlled physical test at one value and restore; verify mode and persistence scope.

## Milestone 5 — RGB zone discovery and controlled effects (implementation complete; visual hardware verification pending)

Tasks: enumerate 8071 clusters/effects; establish zone mapping; static color per reported cluster; effects and ownership/release; deduplicate writes.

Dependency: resolve software-control byte discrepancy and restore path on target hardware.

Outcome: capability-driven 8071 inspection and Off/Static/Cycle/Wave/Breathing setters through HID++ transport, with persistence behavior unverified.

Acceptance: software parsing and request bytes are mock tested. User hardware readout reports one cluster. A setter ACK was observed but no visible light, leading to a corrected software-control request sequence. Visual output, persistence semantics, and non-static modes remain unverified. Read-back is unavailable; capability fields are not treated as persistence proof.

Tests: scripted discovery, effect layouts, software-control request and exact setter bytes; physical visual and persistence checks are still required before claiming behavior. Unknown effect IDs remain display-only and are not given setter support.

## Milestone 6 — Profile read and format decoder (offline format-4 decoder implemented; behavioral meanings partly unresolved)

Tasks: capture complete read-only descriptor/directory/sectors; identify format against provenance and validate checksum; implement raw backup and decoder for exact G102 format only.

Evidence: user-provided `g102-backup.obk` with reported G102 descriptor.

Outcome: offline `onboard decode FILE` validates container structure, checksum on profile sector, and decodes format-4/base-v1 candidate fields while preserving raw bytes.

Acceptance: backup container and two programmed-sector CRCs validate; descriptor-scoped decoder and synthetic fixtures are covered. Physical button order and action behavior remain unverified.

Tests: mock protocol reads, strict container bounds, actual backup fixture, CRC validation and synthetic format-4 sector.

## Milestone 7 — Guarded persistent profile writer and buttons

Tasks: implement copy-on-write format codec; typed button actions, DPI stages, polling fields, G-Shift/macro only if established; backup, preflight, transaction, verify, restore.

Dependency: exact G102 schema plus reviewed read-only decoder and transaction protocol. Unsupported format fails closed.

Outcome: mouse-resident button assignment that survives app exit, disconnect/reboot and another computer.

Acceptance: no write to unknown format; original checksum valid; backup recoverable; unknown bytes preserved; target readback exact; test remapping physically and after reboot/other host.

Tests: format fixtures, byte-preservation property tests, checksum tests, mocked write failures at each phase, physical sacrificial/non-active profile test and restoration.

## Milestone 8 — Palette interface and Wallust adapter

Tasks: generic provider, JSON schema/version, stable file watcher, color mapping, manual overrides and write diff.

Dependency: RGB service; Wallust template variables confirmed for supported version.

Outcome: wallpaper palette sync without hard Wallust dependency.

Acceptance: repeated/partial/atomic updates safe; last valid state retained; unchanged values do not produce device writes; missing Wallust graceful.

Tests: parser/event burst/partial-write/manual precedence/device offline tests; hardware only after RGB milestone.

## Milestone 9 — Background monitoring and interface

Tasks: reconnect service, multiple-device selection, structured CLI; then decide whether GUI/daemon is warranted.

Dependency: stable library APIs.

Outcome: usable Linux-first application.

Acceptance: robust unplug/replug, per-device state, clean shutdown, diagnostics and no implicit flash mutation.

Tests: mock multi-endpoint and reconnect; physical long-running test.

## Optional future work

Other Logitech G devices, receiver child devices, Bluetooth, more 2202/8061/8070 variants, macro editor, G-Shift UX, GUI and additional palette providers. Each needs a device capability record and hardware evidence; none is required to deliver a safe first G102 library.
