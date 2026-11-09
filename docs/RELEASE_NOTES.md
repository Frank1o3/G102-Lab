# Release notes

## 0.1.0 — initial public release candidate (not published)

### Included

- Linux Rust CLI for runtime HID++ discovery and read-only device inspection.
- DPI and polling-rate read/set commands and capability-driven RGB discovery/set commands.
- Raw onboard profile inspection/export and offline `G102OBK1` decode/remap/diff tools.
- Hard-disabled physical onboard profile writes; `--confirm-write` cannot override the validation gate.
- Optional versioned TOML config, narrow Arch udev rule, Arch package recipe, and opt-in Wallust RGB adapter.

### Evidence and limitations

- The supplied G102 backup supports only its documented offline format-4 decoder and CRC checks; it does not validate physical writes.
- DPI, polling-rate, and RGB setters have mock coverage but remain physically unverified for the target model/firmware. Use `VALIDATION.md` before attempting them.
- A `046d:c092` G102 LIGHTSYNC HID++ collection on interface 1 was observed in this environment. PID names conflict across references; the udev rule applies only to that observed ID/interface and does not establish universal device support.
- RGB setter acknowledgement does not prove visible lighting or persistence. No onboard profile writes are supported.
- Actual non-root HID access after installing/reloading the udev rule remains to be verified on each target system.

## Versioning strategy

Use Semantic Versioning once the public command/configuration interfaces stabilize. While the major version is `0`, breaking CLI or config changes may occur in minor releases and must be documented. Keep Cargo's package version, PKGBUILD `pkgver`, and release notes aligned. Tag and publish only through an explicit maintainer action after CI/package validation and hardware-evidence review; this project does not publish releases automatically.
