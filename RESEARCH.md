# Engineering research

Evidence is from checked-out references. “Verified” means a reference reports physical testing or includes a device trace; it was not repeated here. The initial research snapshot did not list a device in `lsusb -nn`; later read-only sysfs/HIDAPI observations identified a `046d:c092` G102 LIGHTSYNC HID++ endpoint on interface 1. No physical setting or onboard-memory writes were issued.

## Workspace discovery

The current workspace root is the `G102-Lab` Git repository, containing the native Rust application and project documentation. The `references/` directory holds research snapshots; those trees are not part of the application package and were not modified. The repository and Cargo project were added after the initial research-only workspace snapshot.

| Directory | Upstream | Branch / commit | Language | License |
| --- | --- | --- | --- | --- |
| references/lghub-linux | <https://github.com/Airp2008/lghub-linux> | master / 6cb4eef | Rust | MIT |
| references/lowtech | <https://github.com/orthory/lowtech> | main / ca4357e | Rust + probes | GPL-2.0-or-later |
| references/OpenGHub | <https://github.com/Slyvan25/OpenGHub> | master / 55822ca | Rust, TypeScript/Svelte | MIT |
| references/OpenLogi | <https://github.com/AprilNEA/OpenLogi> | master / 59ac734 | Rust + UI assets | MIT OR Apache-2.0 |
| references/g203-led | <https://github.com/smasty/g203-led> | master / f9d10ba | Python | MIT |
| references/g102-lightsync-control | <https://github.com/ste4ik/g102-lightsync-control> | main / 19ee3a2 | Python, Qt | MIT; PySide6 LGPLv3/icon ISC noted |
| references/Solaar | <https://github.com/pwr-Solaar/Solaar> | master / e7304c4 | Python | GPL-2.0-or-later |
| references/logi-ghub-linux | <https://github.com/theo-antonescu/logi-ghub-linux> | main / 021f4a2 | Python | GPL-2.0-or-later |
| references/cpg-docs | <https://github.com/Logitech/cpg-docs> | master / f9107a4 | reStructuredText | No license file found |
| references/mouse-protocol | <https://github.com/OpenMouse-Project/mouse-protocol> | main / b62a1cc | TypeScript/docs/captures | AGPL-3.0 |

## Repository comparison

| Project and actual scope | Hardware evidence | Relevant files and value |
| --- | --- | --- |
| lghub-linux: Rust HID++ CLI | README reports wired G102/G203 LIGHTSYNC c092 verified; 2202 and 1b04 explicitly unvalidated. | src/hid.rs, src/hidpp.rs, src/feature.rs, src/dpi.rs, src/features/{report_rate,rgb,onboard,keys}.rs, src/main.rs. Best compact G102-oriented Rust baseline. |
| lowtech: Rust HID++ and probe scripts | PRO X SUPERLIGHT 2 format-7 profile empirically dumped, not G102. | src/hidpp.rs, src/onboard.rs, tools/onboard_dump.py. Strong format gate/safety pattern; offsets not transferable. GPL. |
| OpenGHub: Rust backend + Svelte app | Registry is display metadata, not proof; some claims are model-scoped (G502 LIGHTSPEED). | src-tauri/src/hidpp/{mod.rs,features.rs,registry.rs,onboard.rs}, src-tauri/src/lightsync.rs. Typed capabilities. |
| OpenLogi: cross-platform Rust agent/library/UI | Fixtures for MX/G keyboards are not G102 hardware tests. | crates/openlogi-hidpp, openlogi-hid, openlogi-device, openlogi-core/src/hid, fixtures. Best architecture/mock boundary; simplify. |
| g203-led: Python/PyUSB lighting utility | Names G203 Prodigy c084 and LIGHTSYNC c092; README says lighting nonpersistent. Three-zone packet and apply step in source. | g203-led.py. Best legacy zone-command evidence, but transport differs from HID++. |
| g102-lightsync-control: Python/hidapi GUI/backends | README says Windows tested, Linux/macOS untested; PID support is project claim. | hid_backend.py and README protocol section. Detailed G102 packet hypothesis to cross-check. |
| Solaar: Linux receiver manager | Broad community device reports; no G102-specific physical test found here. | lib/logitech_receiver/{base.py,device.py,hidpp20.py,hidpp20_constants.py}, tests. Receiver slots/events/quirks. GPL. |
| logi-ghub-linux: Python daemon/profile manager | Evidence for G502 LIGHTSPEED, and distinct G915X profile model, not G102. | src/logi_ghub/hidpp/{transport.py,device.py}, onboard_eq.py, actions.py, profiles.py, tests. Strong profile transaction safety. GPL. |
| cpg-docs: Logitech HID++ protocol docs | Documentation, not model-specific testing; this clone has detailed Root docs, not all feature schemas. | hidpp20/README.rst and features/0x0000-IRoot.rst. Primary framing/discovery source. |
| mouse-protocol: community research/captures | Broad profile catalogue explicitly says reverse-engineered/unverified; individual hardware evidence separated. No G102 layout. | docs/logitech-onboard-profiles.md. Provenance discipline. AGPL. |

Subsystem guide: lghub-linux for direct G102 Rust HID++; OpenLogi for architecture/mocks; g203-led for legacy three-zone USB; lghub/OpenGHub/G102 app for HID++ 8071; logi-ghub-linux plus mouse-protocol/lowtech for profile safety and format caution; Solaar/logi-ghub-linux for receivers.

## HID++ protocol

Logitech CPG hidpp20/README.rst: short report 0x10 is 7 bytes; long 0x11 is 20. Layout: report ID, device index, feature index, function/software byte, parameters. Short has 3 parameter bytes; long 16. Feature ID (e.g. 0x2201) is a 16-bit discovery identifier; feature index is a runtime byte in packets. Function is high nibble; software ID is low nibble. Root (0x0000) is mandatory at index 0. Root fn 0 GetFeature takes BE feature ID and returns index, flags and version; zero means absent. Root fn 1 returns protocol version and supports ping. CPG lists major 0x8f HID++ 1.0, 0x02 legacy 2.0, 0x04 2.0. FeatureSet 0x0001 can enumerate; feature version starts at zero.

Error reply sub-ID 0xff identifies the failed request and codes include invalid argument/range, hardware, invalid feature/function, busy, unsupported. Match endpoint/device index/feature/function/software ID and report kind. Notifications can interleave. Serialize requests and reads per endpoint; dispatch unsolicited events. Timeouts are policy: lghub-linux uses 200ms and up to three waits, not a standard. Retry safe reads only. Timeout after a write is ambiguous; don't blindly repeat non-idempotent operations.

Direct wired commonly uses device index 0xff. Receiver-attached devices may use slots 1..6 (lowtech) or Linux logitech-dj child hidraw nodes. Bluetooth may lack short reports. logi-ghub-linux recognizes reports 0x10/0x11/0x12 (7/20/64 bytes); lghub focuses 0x10/0x11. HID++ commonly uses /dev/hidrawN vendor usage page 0xff00. G102-control says its mouse accepts ordinary 20-byte hidapi write/read report 0x11, not feature reports. g203-led instead uses PyUSB class control transfer 0x21/0x09, interface 1, endpoint 0x82 read: different transport.

Comparison: lghub uses hidapi read/write and serialized request loop; logi-ghub-linux discovers report descriptors/sysfs and accesses raw hidraw; OpenLogi separates HID++ framing from raw output via backend traits. Keep transport behind one endpoint interface and validate report-ID conventions.

## Feature research

CPG local docs do not establish every requested feature payload. Do not invent layouts or assign meaning to unknown bytes.

| Feature | Source-established functions/layout | Status |
| --- | --- | --- |
| 0x2201 DPI | lghub src/dpi.rs: fn 0 sensor count, 1 DPI list, 2 current/default, 3 set; DPI BE u16. OpenGHub agrees. | G102 sources choose it. Exact range, step, stage behavior and persistence unknown. |
| 0x2202 Extended DPI | lghub src/features/dpi_2202.rs labels IDs experimental/unvalidated. | Distinct codec; no G102 evidence. |
| 0x8060 Report rate | lghub src/features/report_rate.rs: fn 0 bitmap list (bit N means N+1 ms), fn 1 current ms, fn 2 set ms. 1ms=1000Hz, 2=500, 4=250, 8=125. | G102 sources use it. G102 app says profile mode may reject change and host-mode switch can persist; read-only verify first. |
| 0x8061 Extended rate | OpenGHub features defines capability/get/set fns 0/1/2 and separate indexed rates. | Not an 8060 alias; no G102 evidence. |
| 0x8070 Color LED | CPG catalogue; G102 app describes fn 0 info, 1 zone, 2 effect, 3 set fallback. | App calls it less-tested. |
| 0x8071 RGB | lghub src/features/rgb.rs: fn 00 query device/cluster/effect; 10 set cluster; 20 multi-LED; 30 NV; 40 LED bin; 50 software control; later sync/power. Query args [cluster,effect,TOI], replies include counts/location/persistency/effect ID/caps/period. Set uses long payload: cluster, slot, 10 effect bytes, suffix. | G102 projects use it; enumerate clusters/effects. Control prelude conflicts between projects. |
| 0x8100 Profiles | lghub onboard.rs: fn 00 descriptor; 10 set mode; 20 get; 30 select profile; 40 current; 50 memory read; 60/70/80 write; b0/c0 DPI stage. Descriptor gives model/format IDs and geometry. | Feature presence is not schema. The later supplied G102 backup establishes a scoped format-4 descriptor/geometry; write behavior remains unverified and gated. |
| 0x8110 Button Spy/Filter | OpenGHub: fn 0 count, 1 start, 2 stop, 3 get remap, 4 set; CPG labels Mouse Button Filter. | Host/software diversion, not persistent remapping; G102 unverified. |
| 0x1b04 Reprogrammable controls | CPG generation 5; lghub keys.rs: count/info/get/set reporting fns 0..3; set long. | lghub marks unvalidated; G102 unknown, distinct from profile flash. |

### G102 IDs, DPI and polling

PID names conflict: g203-led maps c084 Prodigy/c092 G203 LIGHTSYNC; G102-control maps c084 Prodigy, c092 G102/G203, c093 G102, c09d G203; lghub calls c092 G102/G203 tested; OpenGHub registry calls c090 G203 and c092 G102. Treat registry names as metadata, not capability evidence.

| PID | References say | Evidence |
| --- | --- | --- |
| 046d:c084 | G203 Prodigy | g203-led and G102 app; older device, don't infer RGB features. |
| 046d:c092 | G203 LIGHTSYNC, G102/G203, or G102 LIGHTSYNC | conflicting naming; lghub reports tested PID, exact revision ambiguous. |
| 046d:c090 | G203 LIGHTSYNC | OpenGHub registry only here. |
| 046d:c093 / c09d | G102/G203 variants | G102 app README calls c093 a G102 variant and c09d a newer G102 revision; the same project's backend maps c09d to G203. This is an internal conflict; Linux test unclear. |

No device was attached/listed, so target PID is unknown. G102 sources choose 2201; README claims max 8000 but exact min/step/stages/default and physical DPI-button behavior are unverified. Query sensor count/list/current/default. Runtime DPI and 8100 selected stage are separate. Don't claim live changes persist.

8060 values are milliseconds mapped to Hz. G102 app says profile mode may own rate and require host mode; host mode itself persists past shutdown according to its README. This is unverified for user's unit. Read current mode/rate first. Whether rate is in G102 flash is unknown.

### Phase 4 onboard inspection and Phase 5 decoder (2026-11-09)

The device report and supplied `g102-backup.obk` establish the descriptor bytes `01 04 01 01 01 06 10 00 ff 0a 01`: memory model 0x01, profile format 0x04, macro format 0x01, one user and one factory profile, six profile buttons, 16 writable sectors of 255 bytes, mechanical layout 0x0a, other-info 0x01. The backup uses `G102OBK1`, container version 1, a 27-byte header, then 16 records of 6-byte (BE ID + BE length) header and 255-byte raw body; total is 4203 bytes with no trailing data. Current-profile response bytes are `00 01 00`; the slot-1 directory entry is `00 01 01 00`, mapping slot 1 to sector 1. Sectors 2..15 are all 0xff.

The last two bytes of both programmed sectors validate as big-endian CRC-16/CCITT-FALSE (poly 0x1021, init 0xffff, no reflection/xorout), over all prior bytes: directory sector 0 is 0x587c and profile sector 1 is 0xb3d1. Common alternatives did not match both. This validates the algorithm against the two programmed sectors in this particular backup, not every G102 revision, profile format or sector state. The checksum remains a validation aid; it does not authorize writes.

The offline decoder is restricted to the exact descriptor above. Its offsets follow the recovered base-v1 schema for format IDs 1–5 in `references/mouse-protocol/docs/logitech-onboard-profiles.md` (it says format 4 is base-v1 plus 0x8070/extras), while the actual G102 bytes corroborate the rate/DPI/color/power/angle/button positions and valid CRC. Decoded raw values: rate byte 1; default/shift stage indexes 1/0; five little-endian DPI values 1000, 1600, 2200, 3200, 4200; color ff ff ff; power and angle-snap bytes ff; six four-byte records at 0x20. First five records are mouse action subtype 1 with masks 1,2,4,8,16. Sixth is special action 0x05 with parameters ff ff. The common reference labels 0x05 as Cycle DPI, but the G102 action has not been behaviorally tested. Stored slot order is known; physical button order/mapping is not verified.

Tentative interpretations only: stored rate byte's units and relation to live 0x8060; default/shifted stage semantics; mouse mask names and special action effect; sentinel meaning of ff settings. Opaque/unknown: format-4 lighting/extras around 0xd0 onward, G-Shift region, profile-name encoding/content (observed bytes are zero), macro use/layout, undocumented fields and physical mapping. The decoder preserves the complete raw backup and does not interpret these regions. `onboard decode FILE` is offline and read-only.

### Phase 6 onboard button remapping preparation (2026-11-09)

Phase 6 investigated the G102 format-4 button table in detail:

1. **Button action offsets and stored ordering**:
   - The G102 descriptor explicitly declares 6 profile buttons (`info.button_count = 6`).
   - The button table starts at offset `0x20` (32 decimal) in profile sector 1, spanning 24 bytes (`0x20..0x38`) with 4 bytes per slot:
     - Slot 1 (0x20..0x24): `80 01 00 01` (Mouse mask 0x0001, Left click)
     - Slot 2 (0x24..0x28): `80 01 00 02` (Mouse mask 0x0002, Right click)
     - Slot 3 (0x28..0x2c): `80 01 00 04` (Mouse mask 0x0004, Middle click)
     - Slot 4 (0x2c..0x30): `80 01 00 08` (Mouse mask 0x0008, Back)
     - Slot 5 (0x30..0x34): `80 01 00 10` (Mouse mask 0x0010, Forward)
     - Slot 6 (0x34..0x38): `90 05 ff ff` (Special 0x05, Cycle DPI)
   - Remaining slots 7..16 in the base-v1 64-byte button block (`0x38..0x60`) are all `0xff`.
   - Stored slot ordering is sequential 1..6, and the supplied backup contains mouse-mask/special-action encodings in those slots. Labels such as Left/Right/Cycle DPI are reference interpretations; behavior and physical button mapping remain unverified without hardware tracing.
2. **Supported action types and parameters**:
   - `0x80 0x01 mm mm`: Mouse button bitmask (16-bit big-endian).
   - `0x80 0x02 mo kk`: Keyboard modifier bitmask and HID usage keycode.
   - `0x80 0x03 uu uu`: Consumer control usage code.
   - `0x90 ff p0 p1`: Special function opcode `ff` with parameters `[p0, p1]`. In G102 backup, opcode `0x05` uses `[0xff, 0xff]`.
   - `0x0s ss aa aa`: Macro invocation at sector `s`, address `a`.
   - `0xff 0xff 0xff 0xff` / `0x00 0x00 0x00 0x00`: Disabled.
3. **Encoding evidence and offline editor policy**:
   - Safely representable: All action types can be losslessly parsed and represented in `ButtonAction`.
   - Accepted encodings: `left`, `right`, `middle`, `back`, `forward`, `dpi-cycle`, and `disable` are accepted by the offline editor based on bytes in the supplied backup and reference schema. This does not verify action behavior or map stored slots to physical buttons.
   - The mouse masks 1..=5 and special tuple `[0x90, 0x05, 0xff, 0xff]` occur in the supplied backup; “DPI Cycle” is a reference label and behavior remains untested. The disabled encoding is recognized by the editor but its physical behavior is not established.
   - Keyboard keypresses, consumer controls, macros, other special opcodes, and unsupported masks remain rejected for editing.
4. **Checksum and profile integrity requirements**:
   - CRC-16/CCITT-FALSE (poly 0x1021, init 0xffff, no reflection/xorout) over bytes 0..252 (253 bytes), stored big-endian at bytes 253..255.
   - When modifying any button slot, ONLY the 4 bytes corresponding to that slot in `0x20..0x38` and the final 2 CRC bytes may change.
   - All other 249 bytes in sector 1 (rate, DPI table, color, power, angle snap, padding, G-shift, name, lighting/extras) and all other 15 sectors (directory sector 0, erased sectors 2..15) remain byte-for-byte identical.
   - The original backup container must never be overwritten.
   - No writes to mouse hardware are implemented.

The descriptor, current directory and six action records establish a scoped offline profile interpretation for the supplied backup. They do not validate physical button behavior or flash writes. A scripted writer implementation exists but is hard-disabled in the CLI and public API pending exact model/firmware validation. 0x8110 remains separate host/software remapping. The local G102/G203 `g203-led` direct USB control-transfer protocol is not used as HID++ evidence. OpenLogi is G502-specific; OMM-derived layout claims are reverse-engineered cross-model references, corroborated here only where noted.

Reference scope: local G102/G203 app documents only mode get/set (and warns mode changes persist), not profile-sector format. OpenLogi's 0x8100 directory/profile logic is explicitly checked on G502 LIGHTSPEED. `mouse-protocol` format layouts/checksums are reverse-engineered from OMM and their captures are specific to other Logitech models. These references alone do not verify G102 layout or CRC; the supplied G102 backup independently validates the checksum and corroborates selected format-4/base-v1 fields as described above.

### G102 RGB and transport distinctions

g203-led set_ls_triple sends three color values tagged zones 1, 2, 3, then send_command emits an extra 0x7b apply command. It is USB control traffic, not proof of 8071 cluster numbers. README says setting is nonpersistent.

### Phase 3 implementation evidence (2026-11-08)

The CLI has a capability-driven 0x8071 inspector and effect setter. Root discovery supplies the runtime feature index; no fixed index or 0x8070 fallback is used. Inspection decodes device/cluster/effect fields and reads software-control/event flags. These fields are reference/protocol-derived unless explicitly identified below as user-provided hardware observations. Setter syntax uses the zero-based cluster and effect-local indexes printed by `rgb effects`, then an RGB hex color. Supported named modes are Off, Static, Cycle, Wave and Breathing, using G102-oriented payload layouts. Period is limited to 1000..=8000 ms and brightness to 1..=100. Unknown target effect IDs 0x000d and 0x000e remain read-only.

Before a setter, code sends the G102-oriented fn 5 software-control sequence `[01,03,05]`, then fn 1 with setter parameter `0x01`. References describe that parameter as volatile, but target display/effect persistence has not been established. This corrects the earlier implementation, which omitted ownership and sent parameter zero. It does not switch 0x8100 mode and emits no 0x8100 onboard-profile requests. Software ownership is left active; the behavior and persistence consequences of this ownership on the exact target are not fully validated. There is no hardware-validated release/session cleanup yet. The G102 app source describes these bytes and layouts for G102/G203 LIGHTSYNC, but only a static request acknowledgement is observed on the user's target; visual application and non-static effect behavior still need confirmation.

The number of clusters returned by the device is authoritative. g203-led's three tagged LED positions and apply command use direct USB control transfers on interface 1 and are not reused as HID++ cluster layouts.

### User-provided G102 0x8071 readout

The user ran `sudo ./target/debug/g102-control rgb effects` on the connected device and supplied this output: one cluster (index 0), location `0x0002`, device NV capabilities `0x0003`, extended capabilities `0x0004`, cluster persistence byte `0x01`; effect IDs/slots were `0x00/0`, `0x01/1`, `0x03/2`, `0x04/3`, `0x0a/4`, `0x0d/5`, and `0x0e/6`. This establishes one currently enumerated independently addressable cluster, identified as Logo by the known location table. It does not establish the advertised physical zone count, nor that cluster persistence capability means settings actually persist.

The user now reports that diagnostics passed on USB VID:PID `046d:c092`, and that an offline remap changed four bytes in one button record plus two checksum bytes while preserving unrelated bytes. This is user-reported device/backup evidence, not an independently observed session. The offline result corroborates a local file transformation only; it does not verify physical action behavior, slot-to-button mapping, or RGB persistence. Exact firmware and connection metadata were not included with this report.

Based on the G102-oriented reference effect table, CLI labels and setters cover Off (0x00), Static color (0x01), Color cycle (0x03), Color wave (0x04), and Breathing (0x0a). IDs 0x0d and 0x0e remain labeled Unknown effect and are rejected. Although the user readout establishes these effect IDs are advertised by their unit, only static color has had a setter acknowledged; output/parameters for the other effects are not physically verified.

The local G102 app reference gives function 5 ownership `[01,03,05]` and effect parameter layouts for static/cycle/wave/breathing. This agrees with the mouse-specific intended usage, while a broader reference describes additional ownership/power variants for other device families. The implementation uses the G102 sequence only; the target's visible behavior remains to be confirmed. No 0x8070 fallback is implemented.

## Onboard profiles and persistent remapping

This is the chief safety blocker. 8100 descriptor exposes model, profile/macro format IDs, counts, button count and sector geometry; it does not make unknown bytes safe.

mouse-protocol docs/logitech-onboard-profiles.md describes directory sector 0 as four-byte entries (BE sector, state, reserved ff), terminated by ff. Its 255-byte format-7 sector needs overlapping final 16-byte read because read-past-end is rejected. Document says broad format catalogue is reverse-engineered from Logitech OMM and not generally hardware-verified.

lowtech src/onboard.rs reports format 7 on PRO X SUPERLIGHT 2: button table offset 48 (older code uses 32), DPI around offset 4, CRC-16/CCITT-FALSE at tail. It was empirically matched to that mouse, not G102. mouse-protocol reports another PRO X SUPERLIGHT 2 firmware returning format 4, proving model-generation assumptions unsafe.

logi-ghub-linux tests/docs show safer transactions for its own devices: full backup, CRC validation, reject invalid/ROM sectors, start/data/end, readback and restore. actions.py describes 4-byte mouse/keyboard/consumer/special/macro/disabled/unassigned action records. G915X docs use a different bank/file layout and CRC32. Button order, G-Shift, macros, DPI and rate storage vary.

CRC-16/CCITT-FALSE (poly 0x1021, init 0xffff) appears in tested mouse formats but isn't universal. Verify checksum coverage and storage order per format. The supplied backup validates selected G102 format-4 data, but the final transfer byte, commit behavior and firmware scope remain unknown. No physical writes are safe now. Future writes require exact model+firmware, recognized format, valid original checksum, full backup, preservation of unknown bytes, verified encoding/checksum, complete transaction, readback equality and restoration plan. Never probe-write unknown or active profile.

### Profile-format comparison recovered in mouse-protocol

The following is a format map from mouse-protocol/docs/logitech-onboard-profiles.md, recovered from Logitech OMM native registration code. The author distinguishes that reverse-engineered layout from hardware validation. IDs 7 and 8 are numeric cases absent from the older managed enum; labels for them are inferred. **None of these layouts is established for G102.**

| Reported numeric format | Component composition per recovered dispatcher | Structural distinctions | Evidence caveat |
| --- | --- | --- | --- |
| 1 | Base v1 | Scalar DPI table; report rate at 0x00; button table 0x20..0x5f; G-Shift table 0x60..0x9f; name 0xa0..0xcf. | Layout recovered from vendor code. |
| 2–3 | Base v1 + x8070 lighting | Same base-v1 button/G-Shift/name regions plus legacy lighting components. | Exact applicability to G102 unknown. |
| 4 | Base v1 + x8070 + extras | Still base-v1 button/G-Shift offsets. A real PRO X Wireless report is separately documented as format 4, 16 sectors of 255 bytes, CRC-valid, with scalar DPI and 1ms report-rate byte. | One different model's sector was hardware-observed; not G102. |
| 5 | Base v1 + x8071 lighting + counters | Base-v1 layout with RGB-effects components/counters. | Vendor-code recovery; not G102 tested. |
| 6 | Base v6 | Separate layout, not extension of v1; wired and wireless report-rate bytes at 0x00/0x01; DPI v6 region starts 0x02; buttons at 0x30; G-Shift at 0x70; name at 0xa0; lighting components from 0xd0; flag 0xfc. | Vendor-code recovery; field-specific tests are on named newer devices. |
| 7 | Base v6 + bunny-hopping component | Same v6 base; lowtech reports button table offset 48 (0x30) in a 255-byte profile. Format-7 DPI stages are 5-byte x/y/LOD records on tested PRO X SUPERLIGHT 2. | Hardware evidence is specific to PRO X SUPERLIGHT 2, not G102. |
| 8 | Base v6 + bunny hopping + analog-button component | Analog-button block identifies HITS/Superstrike family by inference; 5 DPI slots and a validated sector are reported on PRO X 3/PRO X 2 Superstrike. | Newer format-8 hardware observations are model-specific; ID-to-name relation partly inferred. |

Base-v1 recovered offsets: 0x00 rate; 0x01..0x0a scalar DPI; 0x0d color; 0x10 power; 0x11 angle snap; 0x20 0x40-byte button table; 0x60 0x40-byte G-Shift table; 0xa0 0x30-byte profile name. Base-v6 offsets: 0x00 wireless rate index; 0x01 wired rate index; 0x02..0x1a DPI v6; 0x1d DPI delta; 0x21 power; 0x22 angle snapping; 0x23 write counter; 0x2c/0x2e timeouts; 0x30 0x30-byte buttons; 0x70 0x30-byte G-Shift; 0xa0 0x30-byte name; 0xd0/0xdb/0xe6/0xf1 lighting components; 0xfc lighting flag. These offsets are not G102 specifications.

The profile doc says profile name storage is UTF-16LE despite an accessor named UTF-8; an all-ff name means unset. The v1 DPI table and v6 DPI stage structure differ: v1 has scalar DPI values, while v6 holds x/y plus per-stage LOD. Do not apply v6 stage count/range or LOD semantics to v1. Device report-rate limits are per transport and model even when a profile field uses an index; a stored index alone does not prove a rate is supported over a particular link.

### 8100 memory operations and action encoding

Across Rust and Python references, common 8100 command shape is descriptor (fn 00), mode set/get (10/20), profile select/current (30/40), memory read (50), memory write transaction (60 start, 70 data chunks, 80 end), and current DPI stage (b0/c0). lghub implementation names these functions at references/lghub-linux/src/features/onboard.rs:6-17 and descriptor/read behavior at 62-130. lowtech implements its writer at references/lowtech/src/onboard.rs:30-34 and corresponding transaction body later in that file. These common opcodes do not establish data layouts for other formats.

Button assignments are commonly represented as four-byte records in the logi-ghub-linux actions model: 0x80 0x01 plus mouse mask; 0x80 0x02 plus modifier/key; 0x80 0x03 plus consumer usage; 0x90 special function; macro references; zero or ff patterns for disabled/default. The recovered profile format also defines special function ordinals including DPI cycle/up/down/default/shift, profile select/cycle, G-Shift and scroll. Treat action bytes as format-specific; preserve any unrecognized tuple exactly. G-Shift can have a parallel action layer, as separate G-Shift tables in base-v1/base-v6 indicate. Macros may be stored in additional sectors and referenced by sector/address; availability and encoding need target-specific evidence.

CRC: mouse-protocol documents CRC-16/CCITT-FALSE over bytes excluding final two, stored big-endian, init ffff/poly 1021 for the discussed OMM formats; captured sectors are reported valid on specific PRO X devices. Do not infer that G102 reports the same checksum simply because it exposes 8100.

## Compatibility

Reusable: packet codec, Root discovery, response/error matching, serialized endpoint worker. Optional separate capabilities: 2201/2202, 8060/8061, 8070/8071, 8100 format, 1b04, 8110. Wired, receiver, Bluetooth and newer devices differ in indexes/reports (including 64-byte report 0x12). Capability discovery and validated format codecs govern availability. Model quirks may select known codecs but cannot replace capability evidence. No universal Logitech claim.

## Wallust

Official Wallust documentation was fetched on 2026-11-09: template variables include `color0`–`color15`, `background`, `foreground`, and `cursor`; default colors render as `#RRGGBB`. Templates use MiniJinja-style `{{ variable }}` syntax. Named template config uses `[templates]` entries with `template`/`target`; `target` may expand `~`. Hooks are TOML shell commands run through `/bin/sh`, so they are arbitrary command execution and remain user opt-in. Relevant pages: <https://explosion-mental.codeberg.page/wallust/templates/variables.html>, <https://explosion-mental.codeberg.page/wallust/templates/syntax.html>, <https://explosion-mental.codeberg.page/wallust/config/template.html>, <https://explosion-mental.codeberg.page/wallust/config/example.html>, and <https://explosion-mental.codeberg.page/wallust/hooks.html>.

Phase 10 implements `integrations/wallust/g102-control-palette.json.j2` and the standard-library `scripts/g102-control-wallust` adapter. The app-owned JSON schema is `org.g102-control.palette/v1`; the template maps Wallust `background`/`foreground` and `color4`/`color5` to semantic fields. The adapter validates the selected `#RRGGBB` color and requires the configured cluster/effect slot to be reported as Static color (effect ID 0x0001) before invoking the existing RGB CLI. It does not choose a zone or effect index for the user, watch files, or call 0x8100. Its test suite uses sample palette files and a fake CLI. Verify syntax against the Wallust release actually installed; the app neither pins nor installs Wallust.

## License and stack

Studying protocol facts differs from copying code/comments/tests. MIT requires notice if copied; Apache-2.0 has license/patent/NOTICE terms; GPL/AGPL carry copyleft obligations. Linking isn't a blanket exception. Implement independently. cpg-docs has no license file: use facts, not substantial prose. Not legal advice.

Licenses: lghub MIT; lowtech GPL-2.0-or-later; OpenGHub MIT; OpenLogi MIT OR Apache-2.0; g203-led MIT; G102 app MIT (PySide6 LGPLv3); Solaar GPL-2.0-or-later; logi-ghub-linux GPL-2.0-or-later; cpg-docs unspecified; mouse-protocol AGPL-3.0.

Minimal stack: hidapi or direct hidraw behind trait. hidapi simplifies access but native/report-ID behavior needs validation; hidraw needs sysfs/descriptor and permission handling. Narrow udev rules; no root. One synchronous worker per endpoint initially; async optional later. Phase 10 uses serde/TOML for config and Python standard-library JSON parsing in the optional adapter; no watcher/coordinator is shipped. Structured errors and logging; don't log serials/raw memory by default. No premature GUI. No Piper/libratbag runtime dependencies.

## Open questions

Require actual G102: PID/revision/firmware; working HID collection/report types; Root features/indexes/versions; DPI list/current/default and physical stage behavior; polling list/current/mode ownership; RGB feature/zones/effects/payloads/software-control bytes/persistence; 8100 descriptor, profile format/model/macro IDs, sector geometry, raw directory/profile/checksum, button encoding/order, G-Shift/macros and reboot persistence. Read-only first; writes disabled.

Remaining system/hardware questions: verify post-install `uaccess` ACL and regular-user HID access on the target seat; record exact G102 revision/firmware and validate DPI/rate setters plus RGB visibility/persistence; verify Wallust hook ordering only for a chosen installed release. Wallust variables/template syntax and its configurable target model were checked against upstream docs for Phase 10. Do not copy unlicensed CPG prose; protocol facts are recorded separately.

## Source locator (repository-relative)

- CPG packet sizes, fields, index semantics: references/cpg-docs/hidpp20/README.rst:12-47, 76-95. Root functions and feature-version response: references/cpg-docs/hidpp20/features/0x0000-IRoot.rst:5-12, 32-65.
- lghub packet construction, software-ID nibble, response wait: references/lghub-linux/src/hidpp.rs:13-85, 145-245. G102 feature catalogue/verification statements: references/lghub-linux/README.md:10-20.
- DPI functions and encoding: references/lghub-linux/src/dpi.rs:1-90. Polling list/current/set: references/lghub-linux/src/features/report_rate.rs:1-51.
- RGB function IDs/software-control bytes: references/lghub-linux/src/features/rgb.rs:6-49; cluster/effect enumeration and fields: 52-147; setter and persistence interpretation: 150-176.
- 8100 functions, descriptor fields, mode values and read layout: references/lghub-linux/src/features/onboard.rs:6-37, 62-130; CRC implementation: 133-146.
- G102 app PID README list: references/g102-lightsync-control/README.md:83-93; source backend PID table: references/g102-lightsync-control/hid_backend.py:24-48. Protocol/transport and feature claims: README.md:163-194. Linux untested statement: README.md:35-40.
- Legacy three-zone USB packet and apply step: references/g203-led/g203-led.py:224-226, 250-270; interface attach/detach: 278-300. Non-persistence statement: references/g203-led/README.md:30-39.
- Format-7 device scope, button offset and CRC scope: references/lowtech/src/onboard.rs:4-25, 36-43. Format-7 directory/read boundary and reverse-engineering provenance: references/mouse-protocol/docs/logitech-onboard-profiles.md:1-18, 100-140, 504-546. Format disagreement warning: same file:580-595.
- OMM profile composition and base layouts: references/mouse-protocol/docs/logitech-onboard-profiles.md:285-360; format 7/8 evidence and transport-specific rate limits: same file:370-430.
- Action encoding: references/logi-ghub-linux/src/logi_ghub/actions.py:1-32, 80-140. G915X distinct profile storage: references/logi-ghub-linux/docs/g915x-profile-management.md:1-36. Safe write tests: references/logi-ghub-linux/tests/test_onboard.py:1-100.
- Receiver/direct HID raw discovery: references/logi-ghub-linux/src/logi_ghub/hidpp/transport.py:1-105. G203 README transport distinction: references/g203-led/README.md:1-39.
- HID++ 0x8100 sector write sequence (start, write chunks, end): references/lowtech/src/onboard.rs:65-80, references/Solaar/lib/logitech_receiver/hidpp20.py:610-630, and references/logi-ghub-linux/src/logi_ghub/features/onboard.py:120-145.

## HID++ 0x8100 write protocol & safety findings (Phase 7 audit / Phase 8 hardening)

Cross-model references (`lowtech`, Solaar, and `logi-ghub-linux`) support a common transaction shape: `FN_START_WRITE = 6`, one or more 16-byte `FN_WRITE_DATA = 7` reports, then `FN_END_WRITE = 8`. This is reference-derived shape, not hardware-tested G102 write behavior. The supplied backup supports a 255-byte G102 sector length and checksum validation; it contains no evidence about the write wire protocol.

The host implementation has been mock-tested with a 255-byte request length, sixteen 16-byte data reports, and an empty end-write request. The 16th data report necessarily includes a 256th transfer slot after the sector's final 15 bytes. No available G102 evidence establishes whether that extra byte is ignored, consumed, or required to be a particular value. Reference implementations and older docs disagree about padding conventions, and reference behavior is not a G102 specification. Therefore Phase 8 removes any implicit `0x00`/`0xff` default and refuses a non-aligned transfer before `FN_START_WRITE` unless a caller explicitly supplies a tail byte. Explicit configuration is not hardware validation.

`FN_END_WRITE` uses an operation-specific 2,000 ms policy default, configurable through `WriteOptions`. That duration is conservative host policy only; no G102 worst-case commit latency is measured. Any error after the commit request is attempted—including timeout, transport disconnect, malformed response, or HID++ device error—is classified as `PotentiallyCommitted` because the G102 firmware's commit/error semantics are unknown. The client never retries; commit ambiguity requires read-only export and comparison before any further recovery decision. A reply returned after the request deadline is rejected, though the transport interface does not timestamp the device's exact on-wire arrival time.

Transaction states are `Uncommitted` (no commit request attempted), `PotentiallyCommitted` (commit outcome unknown), `Committed` (acknowledged but not yet verified), and `Verified` (readback checks passed). Scripted tests model both an applied commit with a lost acknowledgement and disconnect after the commit request; they prove only host/mock behavior.

Physical profile writes are disabled in public write APIs and CLI until exact G102 model/firmware validation is documented. The former `--confirm-write` check did not satisfy that evidence gate and no longer enables mutation. Read-only inspection/export and offline backup operations remain separate. No Phase 8 key-remapping feature was added.

Hardware-dependent uncertainties still open: (1) acceptance/meaning of the 256th transfer byte; (2) maximum `FN_END_WRITE` latency; (3) whether a matched device error can follow a partial or completed flash commit; (4) whether volatile staging is discarded on endpoint disconnect, USB removal, or power cycle; and (5) exact PID/revision, firmware, and connection mode to which any future finding applies. No physical writes were conducted during Phase 8. These questions require target-specific captures/validation and must not be resolved by assumption.
