# Proposed architecture

The design is provisional until the G102's endpoint, feature table, RGB ownership behavior and onboard format are observed. Implement the reliable read-only device library first; no UI framework is selected.

## Boundaries

```text
CLI / future UI / service
        │ typed commands and events
Application coordinator ─ Configuration ─ Palette providers
        │                                 (Wallust JSON adapter)
Device manager ─ hotplug, identity, reconnect, multiple devices
        │ one serialized worker per endpoint
Device capability model + feature services
        │
HID++ feature codecs ─ DPI / rate / RGB / onboard / remap
        │
HID++ packet codec and dispatcher
        │
HID transport trait ─ hidapi or Linux hidraw
```

### Transport and packet layer

Transport owns one HID endpoint and provides report read/write, timeout, descriptor identity and close semantics. It knows nothing about DPI, RGB or models. Packet layer validates report type/length, device index, feature index, function/software-ID nibble, parameters and error reports. Response dispatcher correlates replies and queues notifications. Short 0x10, long 0x11, and any additional report kind are explicit transport capabilities; never silently resize.

### Device manager and capabilities

Each attached endpoint has a stable internal connection ID, OS path, VID/PID, product string, interface/usage, bus and connection generation. Serial may be stored privately for identity but not logged. Probe read-only: Root protocol version and feature descriptors. Keep feature ID → runtime index/version per generation. Capability record contains supported codec variants and validated ranges (e.g. DPI sensors, supported rate list, lighting clusters/effects). Unknown features are absent from user controls. USB PID is descriptive metadata only.

Receivers can represent a parent plus child logical devices/slots or kernel-created child nodes. Normalize each into a logical HID++ endpoint with correct device index and parent information. Multiple devices each get independent queues. On unplug, cancel pending calls, close handle, invalidate feature/cache state; on reconnect, reopen and rediscover rather than reuse stale indexes.

### Serialized request worker

Only one task/thread reads a physical HID++ channel. Callers submit typed requests through a queue; worker assigns software ID, writes report, reads until matching reply/deadline, dispatches notifications and buffers unrelated replies if appropriate. Timeout/error terminates that request with context. Use bounded safe-read retry policy. For writes, represent timeout as outcome-unknown; do not auto-retry. Close wakes pending callers. No concurrent direct reads from service code.

### Feature services

Feature codec receives the discovered index/version and raw transport client, then returns validated domain values. It is generic across devices implementing that exact feature generation. No product IDs inside codecs. Device quirk/capability policy selects separately validated codecs where feature variants differ.

- DPI: enumerate sensors and ranges; read current/default; setters require supported value. Do not equate runtime DPI and profile stage.
- Polling: separate 8060/8061 codecs; present listed rates only. A host/onboard mode transition is explicit and potentially persistent, never an implicit setter side effect.
- Lighting: represent enumerated zones/clusters and effects (IDs separately from local slots). Separate software-control requests, device-reported persistence capabilities, and observed persistence outcomes. Do not infer persistence from capability flags or setter acknowledgements; preserve prior ownership and provide explicit release/recovery when validated.
- Onboard: descriptor, directory and sector raw bytes first. Decode only a registered, evidence-backed format codec. Read-only interfaces precede any writer interface. Physical profile writes remain disabled until exact G102 model/firmware validation; confirmation alone cannot override the gate. Non-aligned transfers require an explicit unverified tail-byte choice, and all post-commit-request errors remain outcome-unknown unless readback resolves them.
- Button assignment: typed semantic action and raw preserved form; separate host remaps from onboard profile actions. Unsupported/unknown actions remain opaque and unchanged.

### Onboard-memory subsystem

Split command protocol from format codec. Memory transport supplies descriptor, read sector and guarded write transaction. Format codec is selected by reported format/model IDs plus validated device scope; it parses directory/profile and edits a copy of original bytes. Unknown bytes are retained. No “best guess” offset by model family. A write plan contains target sector, original digest/bytes, modified bytes, checksum result, decoded diff and supported format evidence. Before transaction re-read and compare original to detect concurrent changes; backup before mutation; validate whole sector and CRC; write start/data/end; read back and compare exact bytes; only then report success. Any mismatch leaves an explicit recovery state and no automatic retry. The current 255-byte G102 geometry does not establish how the final extra transfer slot is handled; no default is allowed. The 2-second commit timeout is policy, not measured firmware behavior. The public writer is hard-disabled until G102-specific validation exists.

### Palette integration and configuration

**Phase 10 shipped behavior:** user configuration is optional versioned TOML at the XDG config path; the CLI applies defaults < config < explicitly supplied CLI values only after a setter command is run. Loading config, startup, and inspection never apply settings. Wallust is an external template producer; its template writes the app-owned versioned JSON palette. The standalone adapter validates the selected RGB color, checks that the configured cluster/effect slot is currently reported as Static color, and invokes the existing RGB CLI only when explicitly run. There is no file watcher, daemon, or automatically installed hook. See `src/config.rs`, `scripts/g102-control-wallust`, and `docs/WALLUST.md`.

**Future architecture only:** a generic PaletteProvider, directory watcher, manual zone override coordinator, and automatic reconnect/application flow are not implemented in Phase 10 and must not be inferred from this release. Any future automatic application requires a separate explicit opt-in and hardware validation.

The current config schema validates preferred USB identity/interface, DPI sensor/value, polling rate, RGB defaults, and optional Wallust palette path/color/target settings. Config is never created or rewritten automatically; raw profile backups remain separate.

### Background service and interface

Device manager emits attach/detach/capability/state events. Coordinator applies desired settings only when device is present and owns needed control; no unprompted flash writes. Interface layer may later be CLI first, then daemon/UI. It communicates via typed commands and subscribes to events; it never accesses transport directly. Resource cleanup closes handles, cancels watchers, releases software RGB ownership when safe, and avoids changing persistent device mode implicitly.

## Data flow

1. Enumerate hidraw candidate and open read-only endpoint.
2. Probe Root, protocol version and feature indexes/versions.
3. Construct capability snapshot; expose only supported read operations.
4. User or palette event creates validated desired-state diff.
5. Coordinator serializes calls through endpoint worker and required feature service.
6. Read back state when supported; update last-confirmed state without inferring persistence from an acknowledgement.
7. On unplug, invalidate snapshot; reconnect starts at step 1.

Feature mode transitions and persistent profile writes are not generic startup behavior. They require explicit operation pathways and hardware gates.
