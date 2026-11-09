# Project instructions for future agents

## Purpose and target

This workspace contains an independent Logitech G102 LIGHTSYNC control application, implemented in **native Rust** and **Linux-first**. The CLI provides HID++ discovery/inspection, explicit DPI/rate/RGB operations, read-only onboard inspection, and offline backup tools. DPI, polling, and RGB physical behavior remains unverified for the target; physical 0x8100 profile writes remain disabled. Phase 10 prepares Arch/CachyOS packaging, optional TOML config, a narrow udev rule, and an explicitly invoked Wallust adapter. No daemon, service, GUI, or frontend is in scope. Treat other Logitech devices as capability-based extensions, not as universally compatible targets.

## Mandatory constraints

- Do not add Piper or libratbag as runtime dependencies. They may be studied as protocol references.
- Do not modify anything under `references/`; do not pull, switch branches, commit, or alter their histories.
- Do not write to physical hardware until the relevant feature and exact model/firmware behavior have passed the read-only and safety gates in `VALIDATION.md`.
- Never write onboard memory when profile format, sector geometry, checksum, or the meaning of bytes is unknown. Preserve unknown bytes, validate the original sector, make a recoverable backup, calculate the verified checksum, write only a validated complete sector, and read it back byte-for-byte.
- Persistent remapping means mouse-resident storage. Do not present host-side interception or 0x8110 software remapping as persistent onboard remapping.
- Never infer a feature's presence or packet layout from marketing model names. Discover Root feature indexes and versions at runtime and gate features on observed capabilities.
- Treat reference claims according to evidence class: hardware-tested, protocol-specified, reverse-engineered, inferred, experimental, or unverified. A registry entry or README claim alone is not hardware proof.
- Consult `RESEARCH.md`, `ARCHITECTURE.md`, `IMPLEMENTATION.md`, `PLAN.md` and `VALIDATION.md` before implementing each subsystem. Update docs when new physical evidence changes assumptions.
- Do not introduce a GUI framework before the device library and transport contracts are stable.

## Engineering conventions

- Keep transport, packet framing, protocol features and device policy separate. Feature implementations must not embed model-specific behavior.
- Use typed feature IDs, feature indexes, function IDs, report kinds, device indexes and validated value types; never conflate feature ID with runtime feature index.
- Serialize request/response traffic per physical HID++ endpoint. Do not let concurrent callers consume one another's responses.
- Return structured errors that preserve OS/HID errors, timeouts, protocol error replies, unsupported capabilities, malformed data and safety refusals.
- Prefer a small dependency set. Justify each direct dependency in terms of a required boundary.
- Keep configuration versioned, validate it on load, and avoid storing secrets.
- Use rustfmt conventions, explicit checked arithmetic for packet and memory offsets, and no unsafe code without a documented necessity and review.

## Testing and evidence

- Add deterministic unit tests for packet encode/decode, response matching, feature discovery, parsing, range validation, checksums and configuration migration.
- Use mock/scripted transports for tests that do not require hardware. Never require hardware in the default test suite.
- Hardware validation must record exact USB VID/PID, product string, firmware, connection mode, feature IDs/indexes/versions, observed request and response bytes, and whether the result was read back.
- Separate read-only probes from mutations. New mutations default to disabled until a documented device/firmware validation exists.
- Unknown response bytes stay opaque. Do not assign semantics to them without traceable evidence.
- Before persistent writes, follow the complete backup, precondition, checksum, transactional write and read-back sequence in `VALIDATION.md`.

## Documentation discipline

Use repository-relative source paths and exact symbol names. Cite evidence and state its scope. Re-check disagreements rather than copying one implementation. Update the unresolved-question list and milestone acceptance criteria as hardware evidence arrives.
