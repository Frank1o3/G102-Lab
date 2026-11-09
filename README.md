# G102-Lab

G102-Lab is a native Rust Linux CLI for discovering and inspecting Logitech HID++ devices, with explicit DPI, polling-rate, and RGB operations. The G102 LIGHTSYNC is the initial target. Capabilities and feature indexes are discovered at runtime; product names are not treated as proof of feature support. There is no daemon, systemd service, GUI, or frontend.

## Hardware scope and evidence

The initial observed endpoint is a Logitech G102 LIGHTSYNC reported as USB `046d:c092`, HID++ usage page `0xff00`, usage `0x0001`, interface 1. Reference projects disagree about the model name for PID `c092`; this observation applies to the inspected unit only. The CLI currently discovers Logitech vendor collections and does not hard-code a supported PID allowlist.

| Evidence class | Current status |
| --- | --- |
| Read-only hardware observation | A `046d:c092` G102 LIGHTSYNC product string and HID++ collection on interface 1 were observed. User-provided diagnostics report runtime feature discovery, including one Logo RGB cluster. Firmware and a full validation record are not available here. |
| Hardware setter evidence | A user-reported RGB static-color request was acknowledged but did not visibly light the mouse. No DPI or polling setter behavior is established. Acknowledgement is not proof of visible output or persistence. |
| Mock-tested / offline | Packet/discovery code and DPI, polling, RGB, and simulated onboard transaction paths have scripted tests. The supplied `g102-backup.obk` supports a strict offline format-4 decoder and CRC checks for that backup only. Offline remapping changes a local file, not mouse storage. |
| Unverified | Physical DPI/rate behavior, RGB visual effects and persistence, firmware scope, button behavior/physical mapping, and the 0x8100 flash-write protocol. |
| Intentionally disabled | Physical `onboard set-button` and `onboard restore` writes. `--confirm-write` cannot bypass the safety gate. No command in this release writes the 0x8100 onboard profile memory. |

Do not infer independently addressable zones from marketing material or another model. Run `rgb effects` and use the exact cluster/effect indexes it reports. `0x8110` software remapping is separate from persistent onboard remapping.

## Installation on Arch Linux / CachyOS

Install build prerequisites from the official repositories, then build and install from a clean checkout:

1. Install `base-devel`, `rust`, `cargo`, `pkgconf`, `systemd-libs`, and `python` (the latter is needed only for the optional adapter/test helper; Wallust itself is optional).
2. Clone the repository and enter `packaging/arch/`.
3. Run `makepkg -si`.

For example, the prerequisite install is `sudo pacman -S --needed base-devel rust cargo pkgconf systemd-libs python`.

The PKGBUILD builds with `Cargo.lock` (`--locked`) from the checkout, using a fresh makepkg build directory. It installs `g102-control` to `/usr/bin`, docs/examples under `/usr/share`, the optional Wallust helper, and a narrow udev rule. `systemd-libs` is the runtime dependency for libudev; Rust, Cargo, `pkgconf`, GCC, and Python are build/test dependencies. No Piper/libratbag runtime dependency is used.

Remove the package with `sudo pacman -Rns g102-control`. Package-owned files, including its udev rule, are removed by pacman. Installation does not create user config or change mouse settings.

## Device permissions (no root CLI)

The package rule matches only `046d:c092`, USB interface `01`, and the HID raw node, and adds the systemd `uaccess` tag. This grants an ACL to the active local seat rather than making the node world-writable or exposing other Logitech devices. The match is based on the observed unit; it is not a universal G102 rule. Devices with another PID or interface are not granted access by it.

After package installation, reload rules and reconnect the mouse:

1. Run `sudo udevadm control --reload-rules`.
2. Unplug and reconnect the mouse.
3. Find the HID raw node and check its properties/ACL (for example `udevadm info --query=property --name=/dev/hidrawN` and `getfacl /dev/hidrawN`). Confirm `ID_VENDOR_ID=046d`, `ID_MODEL_ID=c092`, and `ID_USB_INTERFACE_NUM=01` on the intended node.
4. As your regular user, run `g102-control inspect`.

The current environment's regular-user probe found the right HID++ node but received `Permission denied` before the rule was installed; successful post-rule access has not been verified. `uaccess` is intended for an active local seat and may not grant access to headless/non-seat sessions. Do not solve this with `chmod 666`, broad `MODE="0666"`, or running the application as root. If access remains unavailable, inspect the matched properties/session ACL and have an administrator choose a narrowly scoped local policy.

## Configuration

Configuration is optional and is never generated or overwritten automatically. Copy the installed example from `/usr/share/g102-control/examples/config.example.toml` to `~/.config/g102-control/config.toml` and edit it. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for all fields and limits.

Use `--config PATH` for an explicit TOML file or `--no-config` to ignore the default. CLI arguments override config. Preferred devices use USB identity/interface fields, not the unstable `--device N` enumeration ordinal. Settings in a config file do nothing until the user explicitly invokes a corresponding setter command.

## CLI examples

```text
g102-control --version
g102-control inspect
g102-control dpi get --sensor 0
g102-control rate list
g102-control rgb info
g102-control rgb effects
g102-control onboard info
g102-control onboard export mouse-backup.obk
g102-control onboard decode mouse-backup.obk
g102-control onboard remap mouse-backup.obk --slot 4 --action right --dry-run
```

Setters remain explicit operations, but physical DPI/rate/RGB behavior is not validated for the target model/firmware. Read [VALIDATION.md](VALIDATION.md) before using them on hardware. For example, after that validation, RGB static color uses the device-reported indexes: `g102-control rgb set <CLUSTER> <STATIC_EFFECT_INDEX> '#89b4fa'`. The CLI checks feature support and setter ranges but cannot prove visual application or persistence.

`onboard decode`, `buttons`, `remap`, and `diff` use local backup files. `onboard set-button` and `restore` are refused before HID initialization unless using their read-only dry-run form; no flag enables the physical writer. Do not interpret a local backup edit as persistent mouse remapping.

## Optional Wallust integration

Wallust supports user-defined templates and documented color variables (`color0`–`color15`, `background`, `foreground`, `cursor`). G102-Lab supplies a template that renders an app-owned JSON palette and a standard-library adapter. The adapter validates the palette, verifies that the configured cluster/effect is currently reported as `Static color (0x0001)`, then invokes the existing RGB CLI. It runs only when explicitly called; there is no watcher or automatically installed hook. See [docs/WALLUST.md](docs/WALLUST.md) for paths, configuration, target selection, and hook caveats.

## Troubleshooting

- **No candidates:** reconnect the mouse; check that Linux exposes the vendor HID++ collection and that the current product/interface is in scope. `inspect` reports candidates and advertised runtime features.
- **Permission denied:** follow the udev reload/reconnect checks above. Do not run the whole CLI as root or broaden access to all hidraw nodes.
- **Multiple candidates:** use `--device N` for the current enumeration or configure a stable VID/PID/interface selector. `--device` indexes can change after reconnect.
- **Feature unsupported:** the CLI gates commands on runtime feature discovery; not every Logitech device/revision advertises the same features.
- **Invalid config:** check TOML syntax, config version 1, exact field names, and value ranges. Unknown keys fail rather than being silently ignored.
- **Wallust does not update RGB:** verify the template's actual `target`, inspect the generated JSON, enable/configure `[wallust]`, and select a reported static-color slot with `rgb effects`. A successful setter response still does not establish visible effect or persistence.
- **Onboard write refusal:** this is intentional. Physical 0x8100 writes remain disabled pending device-specific validation; `--confirm-write` cannot override it.

## Build and development

Rust 1.85+ is required (edition 2024). The lockfile pins dependency versions; use `--locked` for reproducible dependency resolution. On Arch/CachyOS install `base-devel`, Rust/Cargo, `pkgconf`, `systemd-libs`, and Python for integration tests. Then run:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --release --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
cd packaging/arch && makepkg --cleanbuild
```

The GitHub Actions workflow runs formatting, Clippy, tests, release build, adapter tests, and an unprivileged Arch package build. Package installation/removal requires administrator privileges and should be tested on a disposable Arch/CachyOS system. CI does not access physical hardware or publish releases.

## Limitations and project documents

The format-4 decoder is scoped to the supplied backup. No physical onboard-profile write is supported. DPI/polling/RGB setter layouts have mock coverage but are not hardware-validated for the target. RGB ownership cleanup and persistence remain unresolved. The current udev rule and PID evidence cover one observed interface only.

See [AGENTS.md](AGENTS.md) for safety constraints, [RESEARCH.md](RESEARCH.md) for protocol/evidence scope, [ARCHITECTURE.md](ARCHITECTURE.md), [IMPLEMENTATION.md](IMPLEMENTATION.md), [PLAN.md](PLAN.md), and [VALIDATION.md](VALIDATION.md). [docs/RELEASE_NOTES.md](docs/RELEASE_NOTES.md) has the unreleased 0.1.0 notes and manual release policy. Reference material under `references/` is not part of the application package and must remain unchanged.
