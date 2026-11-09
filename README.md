# G102 Control

An independent, native Rust application foundation for inspecting Logitech HID++ devices on Linux, with the G102 LIGHTSYNC as the initial target.

The intended capability set is DPI, polling rate, capability-driven LIGHTSYNC, read-only onboard profile inspection/backup, and later palette synchronization. The advertised three lighting zones are not assumed to be independently addressable. Hardware and firmware differences must be discovered at runtime and handled by capabilities.

## Current status

Phases 1–5 are implemented in software: Linux hidapi discovery, short/long packet encoding, serialized request/response handling, dynamic Root/FeatureSet discovery, DPI/polling commands, 0x8071 RGB capability inspection and volatile effect setters, and read-only 0x8100 profile inspection/raw backup. The user reports the c092 exposes 0x8071 at index 0x0e and 0x8100 at 0x0f; RGB discovery returned one Logo cluster and effects 0x00, 0x01, 0x03, 0x04, 0x0a, 0x0d and 0x0e. A prior static RGB request was acknowledged but did not visibly light the mouse; corrected ownership/volatile handling has not yet been confirmed visually. The local Phase 4 hardware read failed because `/dev/hidraw3` was unavailable. The supplied `g102-backup.obk` now establishes the exact format-4 descriptor, 16 x 255-byte writable sector geometry, six button records, and CRC validation for its two programmed sectors. Layout interpretations remain evidence-scoped; physical behavior and button order are unverified. No onboard memory, profile or mode writes are implemented.

Run `cargo run -- inspect` to enumerate Logitech vendor HID++ collections and inspect their advertised feature tables. RGB commands are `rgb info`, `rgb effects`, and `rgb set <CLUSTER> <EFFECT_INDEX> <RRGGBB>`; cycle, wave and breathing accept optional period/brightness/direction settings. Their writes claim software control and use volatile RAM; visible behavior still needs confirmation. Onboard commands are `onboard info`, `onboard profiles`, `onboard export <FILE> [--device N]`, and offline `onboard decode <FILE>`. Export creates a versioned `G102OBK1` container; the decoder supports only the observed G102 format-4 descriptor and preserves opaque regions. Nothing writes onboard memory or changes operating mode.

Software validation uses `cargo fmt`, `cargo check`, `cargo test`, and `cargo build`. Persistent remapping is structurally plausible but not safe to implement until action meanings, physical button order, and a G102 writer/readback procedure are validated; 0x8110 is treated as a separate host-side feature and is not called persistent remapping.

## Documentation

- [AGENTS.md](AGENTS.md): persistent constraints for future coding sessions
- [RESEARCH.md](RESEARCH.md): workspace, repository, protocol, hardware, compatibility and licensing evidence
- [ARCHITECTURE.md](ARCHITECTURE.md): proposed subsystem boundaries and data flow
- [PLAN.md](PLAN.md): phased implementation milestones
- [IMPLEMENTATION.md](IMPLEMENTATION.md): Rust interfaces and behavior specification
- [VALIDATION.md](VALIDATION.md): unit, mock and physical-device validation

Reference clones are under `references/`; they must remain unchanged during implementation.
