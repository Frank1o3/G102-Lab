# Configuration

Configuration is optional. The CLI works with built-in defaults when no config file exists.

## Location and loading

Default path: `$XDG_CONFIG_HOME/g102-control/config.toml`, or `~/.config/g102-control/config.toml` when `XDG_CONFIG_HOME` is unset/non-absolute. Select a different file with `--config PATH`; use `--no-config` to ignore the default. These global options may appear before or after a command. An explicitly named missing file is an error. The application never creates, rewrites, or migrates a user config automatically.

Copy `config.example.toml` from the source checkout or `/usr/share/g102-control/examples/config.example.toml`, then edit it yourself. Config files are TOML version 1. Unknown keys and unsupported versions are rejected to catch typos and future schema mismatches.

## Settings

- `[device]`: optional `vendor_id`, `product_id`, and `interface_number` form a preferred endpoint identity. Vendor and product IDs must be specified together. This avoids persisting the unstable enumeration ordinal used by `--device N`. Use `inspect` output for the connected endpoint; IDs do not guarantee feature support.
- `[dpi]`: `sensor` defaults to 0; optional `value` is used only when `dpi set` is explicitly invoked without a positional DPI value.
- `[rate]`: optional `hz` is used only by an explicit `rate set` with no positional value.
- `[rgb]`: optional `cluster`, `effect_index`, and `color` fill missing `rgb set` positionals. `period_ms` defaults to 3000, `brightness` to 100, and `direction` to `right`; explicit options override these values. Cluster/effect indexes must be read from `rgb effects`.
- `[wallust]`: optional palette file, semantic color key, cluster, and effect slot configure the separate explicit integration script. `enabled` defaults to false. These settings never trigger device access or application by themselves.

Supported ranges: DPI 1–65535 (the device's discovered range is checked by the setter); polling rate 1–1000 Hz (the device-advertised set is checked); RGB cluster 0–31 and effect index 0–63 (actual reported indexes are checked by the CLI); RGB values are exactly six hexadecimal digits, optionally prefixed by `#`; period is 1000–8000 ms; brightness is 1–100; direction is `left` or `right`. Wallust color keys are `background`, `foreground`, `accent`, and `secondary`.

## Precedence and safety

Built-in defaults < loaded config < explicitly supplied CLI arguments. Missing config is normal; malformed, unreadable, unsupported-version, or out-of-range configuration returns a clear error and non-zero status. A config value is only a default for a command the user explicitly runs: loading config, `inspect`, `dpi get`, `rate get/list`, and `rgb info/effects` do not apply settings. DPI, polling, and RGB setters are implemented but physical behavior remains unverified for the target; follow `VALIDATION.md`. Physical onboard `set-button` and `restore` writes remain disabled and `--confirm-write` cannot override the gate.
