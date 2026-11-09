# Optional Wallust integration

The adapter turns a Wallust-generated palette into one explicitly requested software RGB change. It is an optional Python 3.11+ standard-library script; the Rust application has no Wallust or Python runtime dependency. There is no watcher, daemon, automatic startup action, or onboard-profile write.

## Palette format and Wallust setup

Wallust's official documentation describes template variables `color0`–`color15`, `background`, `foreground`, and `cursor`; color values render as `#RRGGBB` by default, though templates can produce other forms. This project provides `integrations/wallust/g102-control-palette.json.j2`, using only the documented variables. It renders the app-owned `org.g102-control.palette/v1` JSON format; this is not a Wallust cache format. The adapter requires the schema and a six-digit RGB color at the selected key, and handles absent, unreadable, malformed, or invalid files without invoking the setter.

Wallust templates and targets are configured by the user. For example, add a named template to `~/.config/wallust/wallust.toml` (copy the template to a user-owned template path if the installed Wallust version cannot read `/usr/share`):

```toml
[templates]
g102 = { template = '/usr/share/g102-control/wallust/g102-control-palette.json.j2', target = '~/.cache/g102-control/wallust-palette.json' }
```

The default target path is an example, not a Wallust default. Set the exact `target` in your Wallust config and the matching `wallust.palette_file` in G102-Lab config. Wallust’s documented template target can expand `~`.

A safe workflow is to run Wallust in the user's wallpaper-change workflow, wait for that command to finish writing the target, then run `g102-control-wallust-sync`. Wallust also supports shell hooks, but hooks execute arbitrary commands through `/bin/sh`; use one only as an explicit opt-in after confirming ordering in the installed Wallust version. No hook is installed or enabled by this package.

## Device mapping and explicit sync

First run `g102-control rgb effects` and record the cluster and local effect slot identified as `Static color (ID 0x0001)`. These indexes are runtime device output, not fixed G102 zones. The current inspected unit reported one Logo cluster; that observation is not universal hardware support. The adapter re-runs `rgb effects` and refuses unless the requested cluster/slot is currently reported as static color. Unknown effects remain read-only.

Configure the optional integration explicitly in `~/.config/g102-control/config.toml`:

```toml
[wallust]
enabled = true
palette_file = '~/.cache/g102-control/wallust-palette.json'
color_key = 'accent'
cluster = 0 # replace with an index reported by `rgb effects`
effect_index = 1 # replace with the reported Static color slot
```

Then, after Wallust has generated the palette, run `g102-control-wallust-sync`. Alternatively pass `--palette`, `--color-key`, `--cluster`, and `--effect-index` explicitly. The script invokes `g102-control rgb set` without a shell, so palette contents cannot become shell arguments. `--device N` can override candidate enumeration selection; otherwise the same CLI device-selection rules/config apply. The actual visible effect and persistence remain hardware-unverified. The RGB path does not call 0x8100 and does not modify onboard profile memory.

## Wallust references

Checked 2026-11-09: [template variables](https://explosion-mental.codeberg.page/wallust/templates/variables.html), [template syntax](https://explosion-mental.codeberg.page/wallust/templates/syntax.html), [template target configuration](https://explosion-mental.codeberg.page/wallust/config/template.html), [configuration sample](https://explosion-mental.codeberg.page/wallust/config/example.html), and [hooks](https://explosion-mental.codeberg.page/wallust/hooks.html). Verify against your installed Wallust release; this project does not pin or install Wallust.
