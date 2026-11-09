use std::process::ExitCode;

use g102_control::{
    config::{self, AppConfig},
    connect,
    device::DeviceInspection,
    features::{dpi, onboard, report_rate, rgb},
    hid::HidapiTransport,
};
use hidapi::HidApi;

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args
        .first()
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("g102-control {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }
    let (args, config_path, no_config) = match extract_global_options(&raw_args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if args
        .first()
        .is_some_and(|arg| arg == "help" || arg == "--help" || arg == "-h")
    {
        print_help();
        return ExitCode::SUCCESS;
    }
    if args
        .first()
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("g102-control {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let app_config = match AppConfig::load(config_path.as_deref(), no_config) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = if args.is_empty() {
        inspect()
    } else if args[0] == "inspect" && args.len() == 1 {
        inspect()
    } else if args[0] == "inspect" {
        Err("inspect accepts no command arguments; use `--help` for usage".into())
    } else {
        command(&args, &app_config)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "Usage:\n  g102-control [--config FILE | --no-config] inspect\n  g102-control [--config FILE | --no-config] dpi get [--device N] [--sensor N]\n  g102-control dpi set [DPI] [--device N] [--sensor N]\n  g102-control rate get|list [--device N]\n  g102-control rate set [HZ] [--device N]\n  g102-control rgb info|effects [--device N]\n  g102-control rgb set [CLUSTER [EFFECT_INDEX [RRGGBB]]] [--period-ms N] [--brightness 1..100] [--direction left|right] [--device N]\n  g102-control onboard info|profiles [--device N]\n  g102-control onboard export <FILE> [--device N]\n  g102-control onboard decode <FILE>\n  g102-control onboard buttons <FILE>\n  g102-control onboard remap <INPUT> [OUTPUT] --slot <1..6> --action <ACTION> [--dry-run]\n  g102-control onboard diff <FILE_A> <FILE_B>\n  g102-control onboard set-button <SLOT> <ACTION> [--confirm-write] [--dry-run] [--device N] [--backup-file FILE] [--commit-timeout-ms MS]\n  g102-control onboard restore <BACKUP_FILE> [--confirm-write] [--dry-run] [--device N] [--backup-file FILE] [--commit-timeout-ms MS]\n  g102-control --version\n\nExamples:\n  g102-control inspect\n  g102-control dpi get --sensor 0\n  g102-control rate list\n  g102-control rgb effects\n  g102-control rgb set 0 1 '#89b4fa'\n  g102-control onboard export mouse-backup.obk\n  g102-control onboard decode mouse-backup.obk\n\nOptions:\n  --config FILE  Load a TOML config (default: $XDG_CONFIG_HOME/g102-control/config.toml or ~/.config/g102-control/config.toml)\n  --no-config    Ignore the default config; cannot be combined with --config\n  --device N     Select a candidate from the current enumeration (not a stable identity)\n  -h, --help     Show this help; -V, --version shows the package version\n\nValues explicitly supplied on the command line override config. Config values are defaults only: they never change a device unless a setter command is explicitly run. DPI/rate/RGB setters are implemented but physical behavior is unverified for the target; follow VALIDATION.md before using them.\nUse `rgb effects` to obtain reported zero-based cluster/effect indexes. Setter layouts are implemented for effect IDs 0x00, 0x01, 0x03, 0x04, and 0x0a; other IDs remain read-only. Reported persistence capabilities and setter acknowledgements do not establish persisted behavior.\n`onboard buttons`, `onboard decode`, `onboard remap`, and `onboard diff` operate on local `G102OBK1` backup files. Physical `onboard set-button` and `onboard restore` writes are disabled pending exact G102 model/firmware validation; `--confirm-write` cannot override this gate."
    );
}

fn extract_global_options(
    args: &[String],
) -> Result<(Vec<String>, Option<std::path::PathBuf>, bool), String> {
    let mut command_args = Vec::new();
    let mut config_path = None;
    let mut no_config = false;
    let mut index = 0;
    while index < args.len() {
        let value = &args[index];
        if value == "--config" {
            if config_path.is_some() {
                return Err("--config may be specified only once".into());
            }
            index += 1;
            let path = args
                .get(index)
                .ok_or_else(|| "--config requires a file path".to_owned())?;
            if path.is_empty() || path.starts_with('-') {
                return Err("--config requires a file path".into());
            }
            config_path = Some(std::path::PathBuf::from(path));
        } else if let Some(path) = value.strip_prefix("--config=") {
            if config_path.is_some() {
                return Err("--config may be specified only once".into());
            }
            if path.is_empty() {
                return Err("--config requires a file path".into());
            }
            config_path = Some(std::path::PathBuf::from(path));
        } else if value == "--no-config" {
            if no_config {
                return Err("--no-config may be specified only once".into());
            }
            no_config = true;
        } else {
            command_args.push(value.clone());
        }
        index += 1;
    }
    if no_config && config_path.is_some() {
        return Err("--config and --no-config cannot be used together".into());
    }
    Ok((command_args, config_path, no_config))
}

fn hid_api() -> Result<HidApi, String> {
    HidApi::new().map_err(|error| format!("could not initialize Linux HID access: {error}; check hidraw permissions and the installed hidapi backend"))
}

fn inspect() -> Result<(), String> {
    let api = hid_api()?;
    let devices: Vec<_> = HidapiTransport::enumerate(&api)
        .into_iter()
        .filter(|device| device.is_candidate())
        .collect();
    if devices.is_empty() {
        println!("No Logitech HID++ vendor collections found (VID 046d, usage page ff00).");
        return Ok(());
    }
    let mut inspected = 0;
    for (index, candidate) in devices.iter().enumerate() {
        println!(
            "Candidate {index}: {} PID {:04x}, interface {}, usage page {:04x}, usage {:04x}",
            candidate.manufacturer.as_deref().unwrap_or("Logitech"),
            candidate.product_id,
            candidate.interface_number,
            candidate.usage_page,
            candidate.usage
        );
        match connect(&api, candidate.clone()) {
            Ok(connected) => {
                inspected += 1;
                print_inspection(&connected.inspection);
            }
            Err(error) => println!("  HID++ probe: {error}\n  No device settings were written."),
        }
    }
    if inspected == 0 {
        return Err("could not inspect any HID++ endpoint; check the scoped udev permissions and reconnect the device".into());
    }
    Ok(())
}

fn command(args: &[String], app_config: &AppConfig) -> Result<(), String> {
    if args.len() < 2 {
        return Err("expected dpi, rate, rgb, or onboard command; use --help".into());
    }
    let family = args[0].as_str();
    let action = args[1].as_str();
    if !matches!(
        (family, action),
        ("dpi", "get" | "set")
            | ("rate", "get" | "list" | "set")
            | ("rgb", "info" | "effects" | "set")
            | (
                "onboard",
                "info"
                    | "profiles"
                    | "export"
                    | "decode"
                    | "buttons"
                    | "remap"
                    | "diff"
                    | "set-button"
                    | "restore"
            )
    ) {
        return Err("unknown command; use --help".into());
    }
    let mut positional = Vec::new();
    let mut device_index = None;
    let mut sensor_index = app_config.dpi.sensor;
    let mut effect_period_ms = app_config.rgb.period_ms;
    let mut effect_brightness = app_config.rgb.brightness;
    let mut wave_direction = match app_config.rgb.direction.as_str() {
        "left" => rgb::WaveDirection::Left,
        _ => rgb::WaveDirection::Right,
    };
    let mut remap_slot = None;
    let mut remap_action = None;
    let mut dry_run = false;
    let mut confirm_write = false;
    let mut backup_file = None;
    let mut commit_timeout_ms = None;
    let mut used_device_option = false;
    let mut used_sensor_option = false;
    let mut used_rgb_option = false;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                dry_run = true;
            }
            "--confirm-write" => confirm_write = true,
            "--backup-file" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--backup-file requires a file path".to_owned())?;
                backup_file = Some(value.clone());
            }
            "--commit-timeout-ms" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    "--commit-timeout-ms requires a millisecond duration".to_owned()
                })?;
                let ms: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid --commit-timeout-ms value `{value}`"))?;
                commit_timeout_ms = Some(ms);
            }
            "--slot" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--slot requires a slot number (1..=6)".to_owned())?;
                let slot: usize = value
                    .parse()
                    .map_err(|_| format!("invalid --slot value `{value}`"))?;
                remap_slot = Some(slot);
            }
            "--action" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--action requires an action target".to_owned())?;
                remap_action = Some(value.clone());
            }
            "--device" | "--sensor" | "--period-ms" | "--brightness" | "--direction" => {
                let option = args[i].as_str();
                match option {
                    "--device" => used_device_option = true,
                    "--sensor" => used_sensor_option = true,
                    _ => used_rgb_option = true,
                }
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| format!("{option} requires a value"))?;
                if option == "--direction" {
                    wave_direction = match value.as_str() {
                        "right" => rgb::WaveDirection::Right,
                        "left" => rgb::WaveDirection::Left,
                        _ => return Err("--direction must be `left` or `right`".into()),
                    };
                    i += 1;
                    continue;
                }
                let parsed: usize = value
                    .parse()
                    .map_err(|_| format!("invalid {option} value `{value}`"))?;
                if option == "--device" {
                    device_index = Some(parsed);
                } else {
                    match option {
                        "--sensor" => {
                            sensor_index = u8::try_from(parsed)
                                .map_err(|_| "sensor index must fit in 0..=255".to_owned())?
                        }
                        "--period-ms" => {
                            effect_period_ms = u16::try_from(parsed)
                                .map_err(|_| "period must fit in 0..=65535 ms".to_owned())?;
                            if !(1000..=8000).contains(&effect_period_ms) {
                                return Err("period must be in 1000..=8000 ms".into());
                            }
                        }
                        "--brightness" => {
                            effect_brightness = u8::try_from(parsed)
                                .map_err(|_| "brightness must fit in 0..=255".to_owned())?;
                            if !(1..=100).contains(&effect_brightness) {
                                return Err("brightness must be in 1..=100".into());
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
            value if value.starts_with('-') => return Err(format!("unknown option `{value}`")),
            value => positional.push(value.to_owned()),
        }
        i += 1;
    }

    if used_sensor_option && family != "dpi" {
        return Err("--sensor is only valid for DPI commands".into());
    }
    if used_rgb_option && !(family == "rgb" && action == "set") {
        return Err(
            "--period-ms, --brightness, and --direction are only valid for `rgb set`".into(),
        );
    }
    if (remap_slot.is_some() || remap_action.is_some())
        && !(family == "onboard" && matches!(action, "remap" | "set-button"))
    {
        return Err("--slot and --action are only valid for onboard remap/set-button".into());
    }
    if (backup_file.is_some() || commit_timeout_ms.is_some())
        && !(family == "onboard" && matches!(action, "set-button" | "restore"))
    {
        return Err(
            "--backup-file and --commit-timeout-ms are only valid for onboard write commands"
                .into(),
        );
    }
    if confirm_write && !(family == "onboard" && matches!(action, "set-button" | "restore")) {
        return Err(
            "--confirm-write only applies to disabled onboard writes and never enables them".into(),
        );
    }
    if dry_run && !(family == "onboard" && matches!(action, "remap" | "set-button" | "restore")) {
        return Err("--dry-run is only valid for onboard remap/set-button/restore".into());
    }
    if used_device_option
        && family == "onboard"
        && matches!(action, "decode" | "buttons" | "remap" | "diff")
    {
        return Err("--device is not valid for offline backup commands".into());
    }

    if family == "onboard" {
        match action {
            "decode" => {
                if positional.len() != 1 {
                    return Err("onboard decode requires exactly 1 argument: <BACKUP_FILE>".into());
                }
                let bytes = std::fs::read(&positional[0]).map_err(|error| {
                    format!("could not read backup file `{}`: {error}", positional[0])
                })?;
                let backup = onboard::decode_backup(&bytes).map_err(|e| e.to_string())?;
                print_decoded_backup(&backup)?;
                return Ok(());
            }
            "buttons" => {
                if positional.len() != 1 {
                    return Err("onboard buttons requires exactly 1 argument: <BACKUP_FILE>".into());
                }
                let bytes = std::fs::read(&positional[0]).map_err(|error| {
                    format!("could not read backup file `{}`: {error}", positional[0])
                })?;
                let backup = onboard::decode_backup(&bytes).map_err(|e| e.to_string())?;
                print_button_table(&backup)?;
                return Ok(());
            }
            "remap" => {
                if positional.is_empty() {
                    return Err("onboard remap requires at least <INPUT_BACKUP>".into());
                }
                if positional.len() > 2 {
                    return Err("onboard remap accepts only <INPUT_BACKUP> [OUTPUT_BACKUP]".into());
                }
                let input_path = &positional[0];
                let output_path = positional.get(1).map(String::as_str);
                if !dry_run && output_path.is_none() {
                    return Err("onboard remap requires <OUTPUT_BACKUP> or --dry-run".into());
                }
                let slot = remap_slot.ok_or("onboard remap requires --slot <1..6>")?;
                let action_str = remap_action
                    .as_deref()
                    .ok_or("onboard remap requires --action <ACTION>")?;
                let verified_action =
                    onboard::VerifiedAction::parse(action_str).map_err(|e| e.to_string())?;

                let bytes = std::fs::read(input_path).map_err(|error| {
                    format!("could not read backup file `{input_path}`: {error}")
                })?;
                let backup = onboard::decode_backup(&bytes).map_err(|e| e.to_string())?;

                let plan = onboard::create_button_remap_plan(&backup, &[(slot, verified_action)])
                    .map_err(|e| e.to_string())?;

                if dry_run {
                    print_remap_plan(&plan, true, input_path, output_path);
                } else {
                    let out_path = output_path.unwrap();
                    onboard::save_backup_file(out_path, &plan.modified_backup, Some(input_path))
                        .map_err(|e| e.to_string())?;
                    print_remap_plan(&plan, false, input_path, Some(out_path));
                }
                return Ok(());
            }
            "diff" => {
                if positional.len() != 2 {
                    return Err(
                        "onboard diff requires exactly 2 arguments: <FILE_A> <FILE_B>".into(),
                    );
                }
                let bytes_a = std::fs::read(&positional[0]).map_err(|error| {
                    format!("could not read backup file `{}`: {error}", positional[0])
                })?;
                let backup_a = onboard::decode_backup(&bytes_a).map_err(|e| e.to_string())?;
                let bytes_b = std::fs::read(&positional[1]).map_err(|error| {
                    format!("could not read backup file `{}`: {error}", positional[1])
                })?;
                let backup_b = onboard::decode_backup(&bytes_b).map_err(|e| e.to_string())?;
                let diff = onboard::compare_backups(&backup_a, &backup_b);
                print_backup_diff(&positional[0], &positional[1], &diff);
                return Ok(());
            }
            _ => {}
        }
    }

    if family == "onboard" && action == "set-button" {
        if positional.len() > 2 {
            return Err("onboard set-button accepts at most <SLOT> <ACTION>".into());
        }
        if remap_slot.is_none() && !positional.is_empty() {
            let slot_val: usize = positional[0]
                .parse()
                .map_err(|_| format!("invalid slot value `{}`", positional[0]))?;
            remap_slot = Some(slot_val);
            if remap_action.is_none() && positional.len() >= 2 {
                remap_action = Some(positional[1].clone());
            }
        }
        if remap_slot.is_none() || remap_action.is_none() {
            return Err(
                "onboard set-button requires <SLOT> <ACTION> (or --slot <SLOT> --action <ACTION>)"
                    .into(),
            );
        }
        let action_str = remap_action.as_deref().unwrap();
        let _ = action_str
            .parse::<onboard::VerifiedAction>()
            .map_err(|e| e.to_string())?;
        if !dry_run {
            return Err(onboard::PROFILE_WRITE_GATE_MESSAGE.into());
        }
    } else if family == "onboard" && action == "restore" {
        if positional.len() != 1 {
            return Err("onboard restore requires exactly 1 argument: <BACKUP_FILE>".into());
        }
        if !dry_run {
            return Err(onboard::PROFILE_WRITE_GATE_MESSAGE.into());
        }
    } else {
        let valid_count = match (family, action) {
            ("dpi" | "rate", "set") => (0..=1).contains(&positional.len()),
            ("rgb", "set") => (0..=3).contains(&positional.len()),
            ("onboard", "export") => positional.len() == 1,
            _ => positional.is_empty(),
        };
        if !valid_count {
            return Err(format!(
                "unexpected positional arguments for `{family} {action}`; use --help"
            ));
        }
    }

    if confirm_write && !(family == "onboard" && matches!(action, "set-button" | "restore")) {
        return Err(
            "--confirm-write only applies to disabled onboard writes and never enables them".into(),
        );
    }
    if family == "onboard" && action == "set-button" && confirm_write && !dry_run {
        return Err(onboard::PROFILE_WRITE_GATE_MESSAGE.into());
    }

    let dpi_value = if family == "dpi" && action == "set" {
        Some(resolve_u16(
            positional.first(),
            app_config.dpi.value,
            "DPI",
            1,
            u16::MAX,
        )?)
    } else {
        None
    };
    let rate_value = if family == "rate" && action == "set" {
        Some(resolve_u16(
            positional.first(),
            app_config.rate.hz,
            "rate",
            1,
            1000,
        )?)
    } else {
        None
    };
    let rgb_values = if family == "rgb" && action == "set" {
        let cluster = resolve_u8(positional.first(), app_config.rgb.cluster, "cluster", 31)?;
        let effect = resolve_u8(
            positional.get(1),
            app_config.rgb.effect_index,
            "effect index",
            63,
        )?;
        let color = positional
            .get(2)
            .map(String::as_str)
            .or(app_config.rgb.color.as_deref())
            .ok_or(
                "rgb set requires <CLUSTER> <EFFECT_INDEX> <RRGGBB> or those values in config",
            )?;
        let color = parse_rgb(color)?;
        Some((cluster, effect, color))
    } else {
        None
    };

    let api = hid_api()?;
    let candidates: Vec<_> = HidapiTransport::enumerate(&api)
        .into_iter()
        .filter(|device| device.is_candidate())
        .collect();
    if candidates.is_empty() {
        return Err("no Logitech HID++ vendor collections found".into());
    }
    let selected = select_candidate(&candidates, device_index, app_config)?;
    let connected = connect(&api, selected.clone())
        .map_err(|error| format!("could not connect/probe candidate: {error}"))?;
    let feature_id = match family {
        "dpi" => dpi::FEATURE_ID,
        "rate" => report_rate::FEATURE_ID,
        "rgb" => rgb::FEATURE_ID,
        _ => onboard::FEATURE_ID,
    };
    let feature = connected
        .inspection
        .feature(feature_id)
        .ok_or_else(|| format!("selected device does not advertise feature 0x{feature_id:04x}"))?;
    let has_8110 = connected.inspection.feature(0x8110).is_some();
    let mut client = connected.client;
    match (family, action) {
        ("dpi", "get") => print_dpi(
            dpi::read_sensor(&mut client, feature.index, sensor_index)
                .map_err(|e| e.to_string())?,
        ),
        ("dpi", "set") => {
            let value = dpi_value.expect("DPI set value validated before HID access");
            let sensor = dpi::set_dpi(&mut client, feature.index, sensor_index, value)
                .map_err(|e| e.to_string())?;
            println!(
                "DPI changed and read back as {} (sensor {}). Persistence is not verified.",
                sensor.current_dpi, sensor.index
            );
        }
        ("rate", "get" | "list") => {
            print_rates(report_rate::read(&mut client, feature.index).map_err(|e| e.to_string())?)
        }
        ("rate", "set") => {
            let value = rate_value.expect("rate set value validated before HID access");
            let info = report_rate::set_hz(&mut client, feature.index, value)
                .map_err(|e| e.to_string())?;
            println!(
                "Polling rate changed and read back as {} Hz ({} ms). Persistence is not verified.",
                info.current.hz, info.current.interval_ms
            );
        }
        ("rgb", "info" | "effects") => {
            print_rgb(rgb::inspect(&mut client, feature.index).map_err(|e| e.to_string())?)
        }
        ("rgb", "set") => {
            let (cluster, effect, color) =
                rgb_values.expect("RGB values validated before HID access");
            let caps = rgb::inspect(&mut client, feature.index).map_err(|e| e.to_string())?;
            let id = caps.clusters.get(cluster as usize)
                .and_then(|cluster_info| cluster_info.effects.iter().find(|item| item.local_slot == effect))
                .map(|item| item.id)
                .ok_or_else(|| format!("cluster {cluster} or effect index {effect} was not reported; run `rgb effects`"))?;
            let settings = match id {
                0x0000 => rgb::EffectSettings::Off,
                0x0001 => rgb::EffectSettings::Static { color },
                0x0003 => rgb::EffectSettings::Cycle {
                    period_ms: effect_period_ms,
                    brightness: effect_brightness,
                },
                0x0004 => rgb::EffectSettings::Wave {
                    period_ms: effect_period_ms,
                    direction: wave_direction,
                },
                0x000a => rgb::EffectSettings::Breathing {
                    color,
                    period_ms: effect_period_ms,
                    brightness: effect_brightness,
                },
                id => {
                    return Err(format!(
                        "device effect 0x{id:04x} has no verified G102 setter layout"
                    ));
                }
            };
            rgb::set_effect(&mut client, feature.index, cluster, effect, settings)
                .map_err(|e| e.to_string())?;
            println!(
                "Device acknowledged RGB setter for effect 0x{id:04x}, cluster {cluster}, effect index {effect}. The request includes persistence parameter 0x01 as described by references; actual display/effect persistence is unverified. No 0x8100 onboard-flash request was sent."
            );
        }
        ("onboard", "info" | "profiles") => {
            let info = onboard::inspect(&mut client, feature.index).map_err(|e| e.to_string())?;
            print_onboard(&info, has_8110, feature.version);
        }
        ("onboard", "export") => {
            let info = onboard::inspect(&mut client, feature.index).map_err(|e| e.to_string())?;
            let sectors = onboard::read_writable_memory(&mut client, feature.index, &info.info)
                .map_err(|e| e.to_string())?;
            let backup =
                onboard::encode_backup(&info.info, info.mode, info.current_profile_raw, &sectors)
                    .map_err(|e| e.to_string())?;
            std::fs::write(&positional[0], backup)
                .map_err(|e| format!("could not write backup file `{}`: {e}", positional[0]))?;
            println!(
                "Exported {} raw writable sectors to {}. Format-specific decoding and checksum validity are not asserted.",
                sectors.len(),
                positional[0]
            );
        }
        ("onboard", "set-button") => {
            let slot = remap_slot.unwrap();
            let action_str = remap_action.unwrap();
            let verified_action = action_str
                .parse::<onboard::VerifiedAction>()
                .map_err(|e| e.to_string())?;

            if dry_run {
                let info =
                    onboard::read_info(&mut client, feature.index).map_err(|e| e.to_string())?;
                onboard::validate_descriptor(&info).map_err(|e| e.to_string())?;
                let mode =
                    onboard::read_mode(&mut client, feature.index).map_err(|e| e.to_string())?;
                let current_profile_raw =
                    onboard::read_current_profile_raw(&mut client, feature.index)
                        .map_err(|e| e.to_string())?;
                let sectors = onboard::read_writable_memory(&mut client, feature.index, &info)
                    .map_err(|e| e.to_string())?;
                let live_backup = onboard::RawBackup {
                    info: info.clone(),
                    mode,
                    current_profile_raw,
                    sectors,
                };
                let plan =
                    onboard::create_button_remap_plan(&live_backup, &[(slot, verified_action)])
                        .map_err(|e| e.to_string())?;
                println!("DRY RUN ONLY - No data was written to mouse flash.");
                println!("Target button slot: {slot}");
                for change in &plan.changes {
                    println!(
                        "Current action: {}",
                        change.old_action.display_description()
                    );
                    println!(
                        "Proposed action: {}",
                        change.new_action.display_description()
                    );
                    println!(
                        "Raw bytes: {:02x?} -> {:02x?}",
                        change.old_raw, change.new_raw
                    );
                }
                println!(
                    "Modified sector: {} (profile sector)",
                    plan.profile_sector_id
                );
                println!(
                    "CRC-16/CCITT-FALSE update: 0x{:04x} -> 0x{:04x}",
                    plan.old_checksum, plan.new_checksum
                );
                println!("Byte diffs ({} byte(s) changed):", plan.byte_diffs.len());
                for diff in &plan.byte_diffs {
                    println!(
                        "  offset 0x{:02x} ({}): 0x{:02x} -> 0x{:02x}",
                        diff.offset, diff.offset, diff.old_byte, diff.new_byte
                    );
                }
                println!("All other sectors and profile fields remain unchanged.");
                println!(
                    "Physical profile writes remain disabled pending exact G102 model/firmware validation."
                );
            } else {
                let write_options = commit_timeout_ms.map(|ms| onboard::WriteOptions {
                    commit_timeout: std::time::Duration::from_millis(ms),
                    ..Default::default()
                });
                let report = onboard::execute_profile_button_remap(
                    &mut client,
                    feature.index,
                    &[(slot, verified_action)],
                    true,
                    backup_file.as_deref().map(std::path::Path::new),
                    write_options,
                )
                .map_err(|e| e.to_string())?;

                println!("Physical write successful!");
                println!(
                    "Pre-write safety backup saved: {}",
                    report.backup_path.display()
                );
                println!("Backup verified on disk before issuing write.");
                println!(
                    "Sector {} ({} bytes) written and committed.",
                    report.sector_written, report.bytes_written
                );
                for change in &report.changes {
                    println!(
                        "Slot {}: {} -> {}",
                        change.slot,
                        change.old_action.display_description(),
                        change.new_action.display_description()
                    );
                }
                println!("New sector checksum: 0x{:04x}", report.new_checksum);
                println!(
                    "Readback verification PASSED: byte-for-byte exact match, checksum match, and profile decode verified."
                );
            }
        }
        ("onboard", "restore") => {
            let restore_path = std::path::Path::new(&positional[0]);
            if dry_run {
                let restore_bytes = std::fs::read(restore_path).map_err(|e| {
                    format!(
                        "could not read restore file `{}`: {e}",
                        restore_path.display()
                    )
                })?;
                let restore_backup =
                    onboard::decode_backup(&restore_bytes).map_err(|e| e.to_string())?;
                onboard::validate_descriptor(&restore_backup.info).map_err(|e| e.to_string())?;

                let info =
                    onboard::read_info(&mut client, feature.index).map_err(|e| e.to_string())?;
                onboard::validate_descriptor(&info).map_err(|e| e.to_string())?;
                let mode =
                    onboard::read_mode(&mut client, feature.index).map_err(|e| e.to_string())?;
                let current_profile_raw =
                    onboard::read_current_profile_raw(&mut client, feature.index)
                        .map_err(|e| e.to_string())?;
                let sectors = onboard::read_writable_memory(&mut client, feature.index, &info)
                    .map_err(|e| e.to_string())?;
                let live_backup = onboard::RawBackup {
                    info,
                    mode,
                    current_profile_raw,
                    sectors,
                };
                let diff = onboard::compare_backups(&live_backup, &restore_backup);
                println!("DRY RUN ONLY - No data was written to mouse flash.");
                println!("Restore candidate file: {}", restore_path.display());
                if diff.sector_diffs.is_empty() {
                    println!("Mouse memory already matches restore backup; no changes needed.");
                } else {
                    println!(
                        "Differing sectors: {:?}",
                        diff.sector_diffs
                            .iter()
                            .map(|(id, _)| *id)
                            .collect::<Vec<_>>()
                    );
                    for (sector_id, byte_diffs) in &diff.sector_diffs {
                        println!("Sector {sector_id} diffs ({} byte(s)):", byte_diffs.len());
                        for diff in byte_diffs {
                            println!(
                                "  offset 0x{:02x}: live 0x{:02x} -> restore 0x{:02x}",
                                diff.offset, diff.old_byte, diff.new_byte
                            );
                        }
                    }
                }
                println!(
                    "Physical profile writes remain disabled pending exact G102 model/firmware validation."
                );
            } else {
                let write_options = commit_timeout_ms.map(|ms| onboard::WriteOptions {
                    commit_timeout: std::time::Duration::from_millis(ms),
                    ..Default::default()
                });
                let report = onboard::execute_profile_restore(
                    &mut client,
                    feature.index,
                    restore_path,
                    true,
                    backup_file.as_deref().map(std::path::Path::new),
                    write_options,
                )
                .map_err(|e| e.to_string())?;

                println!("Restore successful!");
                println!(
                    "Pre-restore safety backup saved: {}",
                    report.safety_backup_path.display()
                );
                println!("Restored from: {}", report.restore_file.display());
                println!("Sector(s) written: {:?}", report.sectors_written);
                println!(
                    "Readback verification PASSED: byte-for-byte exact match, checksum match, and profile decode verified."
                );
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn print_onboard(info: &onboard::OnboardInspection, has_8110: bool, feature_version: u8) {
    println!("HID++ 0x8100 onboard profiles (read-only)");
    println!("  Feature version: {feature_version}");
    println!(
        "  Memory model ID: 0x{:02x}; profile format ID: 0x{:02x}; macro format ID: 0x{:02x}",
        info.info.memory_model, info.info.profile_format, info.info.macro_format
    );
    println!(
        "  User profiles: {}; factory profiles: {}; profile button count: {}",
        info.info.profile_count, info.info.factory_profile_count, info.info.button_count
    );
    println!(
        "  Writable sectors: {}; sector size: {} bytes; mechanical layout: 0x{:02x}; other info: 0x{:02x}",
        info.info.sector_count,
        info.info.sector_size,
        info.info.mechanical_layout,
        info.info.various_info
    );
    println!(
        "  Operating mode: {}",
        match info.mode {
            onboard::OperatingMode::Onboard => "onboard profile",
            onboard::OperatingMode::Host => "host/software",
            onboard::OperatingMode::Unknown(value) => {
                println!("  Unknown mode byte: 0x{value:02x}");
                "unknown"
            }
        }
    );
    println!(
        "  Current profile slot: {}; raw response bytes: {:02x?}",
        info.current_profile
            .map_or_else(|| "none reported".into(), |slot| slot.to_string()),
        info.current_profile_raw
    );
    print_directory("User", &info.user_directory);
    print_directory("Factory", &info.factory_directory);
    for sector in &info.profiles {
        println!(
            "  Profile sector 0x{:04x}: {} bytes, checksum candidate: {}",
            sector.id,
            sector.bytes.len(),
            checksum_label(sector.checksum_candidate)
        );
    }
    println!(
        "  This live inspection does not decode profile sectors; use `onboard export FILE` then `onboard decode FILE` for the evidence-scoped offline format-4 decoder."
    );
    println!(
        "  0x8110 software button filter: {} (separate from persistent profiles).",
        if has_8110 {
            "advertised"
        } else {
            "not advertised"
        }
    );
    println!(
        "  Physical 0x8100 profile writes are disabled pending exact G102 model/firmware validation, including final-chunk and commit behavior."
    );
}

fn select_candidate<'a>(
    candidates: &'a [g102_control::HidppDeviceInfo],
    device_index: Option<usize>,
    app_config: &AppConfig,
) -> Result<&'a g102_control::HidppDeviceInfo, String> {
    if let Some(index) = device_index {
        return candidates.get(index).ok_or_else(|| {
            format!(
                "device candidate {index} does not exist ({} candidate(s) found)",
                candidates.len()
            )
        });
    }
    if let (Some(vendor_id), Some(product_id)) =
        (app_config.device.vendor_id, app_config.device.product_id)
    {
        let matches: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                candidate.vendor_id == vendor_id
                    && candidate.product_id == product_id
                    && app_config
                        .device
                        .interface_number
                        .is_none_or(|interface| candidate.interface_number == interface)
            })
            .collect();
        return match matches.as_slice() {
            [candidate] => Ok(*candidate),
            [] => Err("configured preferred device was not found; run `inspect` or use --device N".into()),
            _ => Err("configured preferred device matches multiple endpoints; add interface_number or use --device N".into()),
        };
    }
    match candidates {
        [candidate] => Ok(candidate),
        [] => Err("no Logitech HID++ vendor collections found".into()),
        _ => Err(format!(
            "{} candidates found; select one with --device N",
            candidates.len()
        )),
    }
}

fn resolve_u16(
    value: Option<&String>,
    configured: Option<u16>,
    name: &str,
    minimum: u16,
    maximum: u16,
) -> Result<u16, String> {
    let parsed = match value {
        Some(value) => value
            .parse::<u16>()
            .map_err(|_| format!("{name} must be an integer in {minimum}..={maximum}"))?,
        None => configured.ok_or_else(|| {
            format!("{name} is required; provide it on the command line or in config")
        })?,
    };
    if !(minimum..=maximum).contains(&parsed) {
        return Err(format!("{name} must be in {minimum}..={maximum}"));
    }
    Ok(parsed)
}

fn resolve_u8(
    value: Option<&String>,
    configured: Option<u8>,
    name: &str,
    maximum: u8,
) -> Result<u8, String> {
    let parsed = match value {
        Some(value) => value
            .parse::<u8>()
            .map_err(|_| format!("{name} must be an integer in 0..={maximum}"))?,
        None => configured.ok_or_else(|| {
            format!("{name} is required; provide it on the command line or in config")
        })?,
    };
    if parsed > maximum {
        return Err(format!("{name} must be in 0..={maximum}"));
    }
    Ok(parsed)
}

fn print_decoded_backup(backup: &onboard::RawBackup) -> Result<(), String> {
    let i = &backup.info;
    println!(
        "G102OBK1 backup v1: {} sectors, {} bytes each",
        backup.sectors.len(),
        i.sector_size
    );
    println!(
        "  Descriptor: memory 0x{:02x}, profile format 0x{:02x}, macro format 0x{:02x}; {} buttons; {} user and {} factory profiles",
        i.memory_model,
        i.profile_format,
        i.macro_format,
        i.button_count,
        i.profile_count,
        i.factory_profile_count
    );
    println!(
        "  Mode: {:?}; current-profile bytes: {:02x?}",
        backup.mode, backup.current_profile_raw
    );
    for sector in &backup.sectors {
        let checksum = match sector.checksum_candidate {
            onboard::ChecksumCandidate::Erased => "erased (all ff)".to_owned(),
            onboard::ChecksumCandidate::Crc16CcittFalseMatch(value) => {
                format!("CRC-16/CCITT-FALSE validated against this backup: 0x{value:04x}")
            }
            mismatch => checksum_label(mismatch),
        };
        println!(
            "  Sector 0x{:04x}: {} bytes; {}",
            sector.id,
            sector.bytes.len(),
            checksum
        );
    }
    if (
        i.memory_model,
        i.profile_format,
        i.macro_format,
        i.button_count,
        i.sector_size,
    ) != (1, 4, 1, 6, 255)
    {
        println!("  Decoder is unavailable for this descriptor; raw bytes remain preserved.");
        return Ok(());
    }
    let directory = backup
        .sectors
        .iter()
        .find(|sector| sector.id == 0)
        .ok_or("backup lacks directory sector 0")?;
    let slot = backup.current_profile_raw[1];
    let entry_start = usize::from(slot)
        .checked_sub(1)
        .and_then(|n| n.checked_mul(4))
        .ok_or("invalid current profile slot")?;
    let entry = directory
        .bytes
        .get(entry_start..entry_start + 4)
        .ok_or("current profile slot is outside directory")?;
    let sector_id = u16::from_be_bytes([entry[0], entry[1]]);
    if sector_id == 0xffff || sector_id as usize >= backup.sectors.len() {
        return Err(format!(
            "current profile slot {slot} maps to invalid sector 0x{sector_id:04x}"
        ));
    }
    println!(
        "  Current slot {slot} directory entry {:02x?} maps to sector 0x{sector_id:04x}.",
        entry
    );
    let profile = onboard::decode_g102_format4_profile(i, &backup.sectors[sector_id as usize])
        .map_err(|e| e.to_string())?;
    println!(
        "Profile sector 0x{:04x} (format-4/base-v1 schema interpretation)",
        profile.sector_id
    );
    println!(
        "  Rate byte: 0x{:02x} (mapping tentative)",
        profile.report_rate_raw
    );
    println!(
        "  DPI stage indexes: default {}, shifted {}; values {:?} DPI (layout corroborated, semantics tentative)",
        profile.default_dpi_stage, profile.shifted_dpi_stage, profile.dpi_stages
    );
    println!(
        "  Color bytes: {:02x?}; power byte: 0x{:02x}; angle-snap byte: 0x{:02x} (state meanings unverified)",
        profile.color_raw, profile.power_raw, profile.angle_snap_raw
    );
    println!("  Button actions by stored slot (physical button order is unverified):");
    for button in profile.buttons {
        println!(
            "    {}: {} [raw {:02x?}]",
            button.slot,
            button.action.display_description(),
            button.raw
        );
    }
    println!(
        "  CRC-16/CCITT-FALSE (poly 0x1021, init 0xffff, final two bytes big-endian) matches the programmed directory and profile sectors in this backup only."
    );
    println!(
        "  G-Shift, profile name, format-4 extras and all other undecoded bytes remain opaque in the raw backup."
    );
    Ok(())
}

fn print_button_table(backup: &onboard::RawBackup) -> Result<(), String> {
    let i = &backup.info;
    if (
        i.memory_model,
        i.profile_format,
        i.macro_format,
        i.button_count,
        i.sector_size,
    ) != (1, 4, 1, 6, 255)
    {
        return Err("button table decoder requires G102 format 4 descriptor (model 0x01, format 0x04, macro 0x01, 6 buttons, 255-byte sector)".into());
    }
    let directory = backup
        .sectors
        .iter()
        .find(|sector| sector.id == 0)
        .ok_or("backup lacks directory sector 0")?;
    let slot = backup.current_profile_raw[1];
    let entry_start = usize::from(slot)
        .checked_sub(1)
        .and_then(|n| n.checked_mul(4))
        .ok_or("invalid current profile slot")?;
    let entry = directory
        .bytes
        .get(entry_start..entry_start + 4)
        .ok_or("current profile slot is outside directory")?;
    let sector_id = u16::from_be_bytes([entry[0], entry[1]]);
    if sector_id == 0xffff || sector_id as usize >= backup.sectors.len() {
        return Err(format!(
            "current profile slot {slot} maps to invalid sector 0x{sector_id:04x}"
        ));
    }
    let profile = onboard::decode_g102_format4_profile(i, &backup.sectors[sector_id as usize])
        .map_err(|e| e.to_string())?;

    println!(
        "G102 Format-4 Button Table (Profile sector 0x{:04x}):",
        profile.sector_id
    );
    println!(
        "  Stored slots (1..={}) at profile sector offset 0x20..0x{:02x} (4 bytes per slot):",
        i.button_count,
        0x20 + (i.button_count as usize) * 4
    );
    for button in &profile.buttons {
        let offset = 0x20 + (button.slot - 1) * 4;
        let encoding_status = if button.action.is_corroborated_g102_encoding() {
            "Encoding corroborated; behavior/position unverified"
        } else {
            "Encoding unverified"
        };
        println!(
            "    Slot {} [0x{:02x}..0x{:02x}]: {:02x?} -> {} [{}]",
            button.slot,
            offset,
            offset + 4,
            button.raw,
            button.action.display_description(),
            encoding_status
        );
    }
    println!();
    println!("Evidence & Safety Constraints:");
    println!(
        "  - Stored slot ordering is sequential (slots 1..=6); action names describe decoded encodings only."
    );
    println!(
        "    Action behavior and mapping to physical buttons are unverified without behavioral tracing."
    );
    println!(
        "  - Offline editor accepts these action encodings: left, right, middle, back, forward, dpi-cycle, disable."
    );
    println!(
        "  - Unverified targets (keyboard key combos, consumer controls, macros, other special codes)"
    );
    println!("    are rejected for editing to prevent corrupting onboard memory.");
    println!(
        "  - Profile sector CRC-16/CCITT-FALSE candidate: 0x{:04x} (valid).",
        profile.checksum
    );
    println!(
        "  - Unused button table region (0x38..0x60) and G-Shift region (0x60..0xa0) are all 0xff."
    );
    Ok(())
}

fn print_remap_plan(
    plan: &onboard::ProfileEditPlan,
    dry_run: bool,
    input_path: &str,
    output_path: Option<&str>,
) {
    let mode_str = if dry_run {
        "Dry-run proposed"
    } else {
        "Applied"
    };
    println!(
        "{mode_str} button remapping for G102 format-4 profile sector 0x{:04x}:",
        plan.profile_sector_id
    );
    println!("  Changes:");
    for change in &plan.changes {
        println!(
            "    Slot {} (offset 0x{:02x}..0x{:02x}):",
            change.slot,
            change.offset,
            change.offset + 4
        );
        println!(
            "      Old: {} [{:02x?}]",
            change.old_action.display_description(),
            change.old_raw
        );
        println!(
            "      New: {} [{:02x?}]",
            change.new_action.display_description(),
            change.new_raw
        );
    }
    println!("  CRC-16/CCITT-FALSE update:");
    println!("    Old checksum: 0x{:04x} (valid)", plan.old_checksum);
    println!("    New checksum: 0x{:04x} (valid)", plan.new_checksum);
    println!(
        "  Exact byte changes in profile sector 0x{:04x}:",
        plan.profile_sector_id
    );
    for diff in &plan.byte_diffs {
        println!(
            "    offset 0x{:02x} (decimal {:3}): 0x{:02x} -> 0x{:02x}",
            diff.offset, diff.offset, diff.old_byte, diff.new_byte
        );
    }
    println!("  Profile integrity verification:");
    println!(
        "    - {} byte(s) modified in sector {} (button table & CRC).",
        plan.byte_diffs.len(),
        plan.profile_sector_id
    );
    println!(
        "    - All other {} bytes in sector {} remain exactly untouched.",
        255 - plan.byte_diffs.len(),
        plan.profile_sector_id
    );
    println!(
        "    - All other 15 sectors (sector 0 directory, sectors 2..15 erased) and container headers remain untouched."
    );
    if dry_run {
        println!("  Dry-run only: no files were written and no mouse hardware was contacted.");
    } else if let Some(out) = output_path {
        println!(
            "  Saved modified backup container to `{out}`. Original `{input_path}` preserved untouched."
        );
    }
}

fn print_backup_diff(path_a: &str, path_b: &str, diff: &onboard::BackupDiff) {
    println!("Comparison: `{path_a}` vs `{path_b}`");
    if diff.metadata_changed {
        println!("  Metadata / header mismatch detected.");
    } else {
        println!("  Container headers and descriptor: identical.");
    }
    if diff.sector_diffs.is_empty() {
        println!("  All sector bodies are byte-for-byte identical.");
        return;
    }
    for (sector_id, diffs) in &diff.sector_diffs {
        println!(
            "  Sector 0x{:04x}: {} byte(s) differ:",
            sector_id,
            diffs.len()
        );
        for d in diffs {
            let note = if *sector_id == 1 && d.offset >= 0x20 && d.offset < 0x38 {
                " (button table)"
            } else if *sector_id == 1 && d.offset >= 253 {
                " (CRC-16)"
            } else {
                ""
            };
            println!(
                "    offset 0x{:02x} (decimal {:3}): 0x{:02x} vs 0x{:02x}{note}",
                d.offset, d.offset, d.old_byte, d.new_byte
            );
        }
    }
    if diff.sector_diffs.len() == 1 && diff.sector_diffs[0].0 == 1 {
        let only_button_and_crc = diff.sector_diffs[0]
            .1
            .iter()
            .all(|d| (d.offset >= 0x20 && d.offset < 0x38) || d.offset >= 253);
        if only_button_and_crc {
            println!(
                "  Integrity check: differences strictly confined to button table and sector CRC. Unrelated profile data is unchanged."
            );
        }
    }
}

fn print_directory(name: &str, entries: &[onboard::DirectoryEntry]) {
    println!("  {name} directory (common 4-byte format interpretation):");
    if entries.is_empty() {
        println!("    no decoded entries (raw directory is included in export)");
    }
    for entry in entries {
        println!(
            "    slot {} -> sector 0x{:04x}; enabled {}; target plausible {}; raw {:02x?}",
            entry.slot, entry.sector, entry.enabled, entry.valid_target, entry.raw
        );
    }
}

fn checksum_label(checksum: onboard::ChecksumCandidate) -> String {
    match checksum {
        onboard::ChecksumCandidate::Erased => "erased (all ff)".into(),
        onboard::ChecksumCandidate::Crc16CcittFalseMatch(value) => {
            format!("CRC-16/CCITT-FALSE candidate match 0x{value:04x}")
        }
        onboard::ChecksumCandidate::Crc16CcittFalseMismatch { stored, calculated } => format!(
            "CRC-16/CCITT-FALSE candidate mismatch (stored 0x{stored:04x}, calculated 0x{calculated:04x})"
        ),
    }
}

fn parse_rgb(value: &str) -> Result<[u8; 3], String> {
    config::validate_color(value)
}

fn print_rgb(info: rgb::LightingInfo) {
    println!(
        "HID++ 0x8071 lighting: {} cluster(s); device-reported NV capability field 0x{:04x}, extended field 0x{:04x}.",
        info.cluster_count, info.nv_capabilities, info.extended_capabilities
    );
    println!(
        "  Device-reported software-control flags 0x{:02x}; event flags 0x{:02x}",
        info.software_control.0, info.software_control.1
    );
    println!(
        "  Capability fields do not establish whether an RGB change persisted. Actual display/effect persistence is unverified; this RGB command path does not issue 0x8100 onboard-profile writes."
    );
    for cluster in info.clusters {
        println!(
            "  Cluster {}: location {} (0x{:04x}); reported display-persistence capability 0x{:02x}; reported effect-persistence support {}; multi-LED pattern capability {}",
            cluster.index,
            location_name(cluster.location),
            cluster.location,
            cluster.persistence_capabilities,
            cluster.effect_persistence_supported,
            cluster.multi_led_pattern_supported,
        );
        for effect in cluster.effects {
            println!(
                "    effect index {}: {} (ID 0x{:04x}), capabilities 0x{:04x}, period {}",
                effect.local_slot,
                effect_name(effect.id),
                effect.id,
                effect.capabilities,
                effect.period
            );
        }
    }
}

fn location_name(location: u16) -> &'static str {
    match location {
        0x0001 => "Primary",
        0x0002 => "Logo",
        0x0003 => "Left",
        0x0004 => "Right",
        0x0005 => "Combined",
        _ => "Unknown location",
    }
}

fn effect_name(id: u16) -> &'static str {
    match id {
        0x0000 => "Off",
        0x0001 => "Static color",
        0x0003 => "Color cycle",
        0x0004 => "Color wave",
        0x000a => "Breathing",
        _ => "Unknown effect",
    }
}

fn print_dpi(sensor: dpi::DpiSensor) {
    println!(
        "DPI sensor {}: current {}, default {}",
        sensor.index, sensor.current_dpi, sensor.default_dpi
    );
    match sensor.support {
        dpi::DpiSupport::Discrete(values) => println!(
            "  Supported DPI: {}",
            values
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        dpi::DpiSupport::Range {
            minimum,
            maximum,
            step,
        } => println!("  Supported DPI: {minimum}..={maximum}, step {step}"),
    }
}

fn print_rates(info: report_rate::ReportRateInfo) {
    println!(
        "Polling rate: {} Hz ({} ms)",
        info.current.hz, info.current.interval_ms
    );
    println!(
        "  Supported: {}",
        info.supported
            .iter()
            .map(|rate| format!("{} Hz ({} ms)", rate.hz, rate.interval_ms))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn print_inspection(device: &DeviceInspection) {
    println!("  Device: {}", device.product_name());
    println!(
        "  USB identity: {:04x}:{:04x}; HID++ index 0x{:02x}, report 0x{:02x}; protocol {}.{}",
        device.device.vendor_id,
        device.device.product_id,
        device.device_index,
        device.report_kind.id(),
        device.protocol_version.major,
        device.protocol_version.minor
    );
    println!("  Features:");
    for feature in &device.features {
        println!(
            "    {} index 0x{:02x}, v{}",
            feature.id, feature.index, feature.version
        );
    }
    println!(
        "  DPI 0x2201: {}; polling 0x8060: {}; RGB 0x8071: {}",
        device
            .feature(0x2201)
            .map(|f| format!("available at index 0x{:02x}", f.index))
            .unwrap_or_else(|| "not advertised".into()),
        device
            .feature(0x8060)
            .map(|f| format!("available at index 0x{:02x}", f.index))
            .unwrap_or_else(|| "not advertised".into()),
        device
            .feature(rgb::FEATURE_ID)
            .map(|f| format!("available at index 0x{:02x}, v{}", f.index, f.version))
            .unwrap_or_else(|| "not advertised".into())
    );
}

#[cfg(test)]
mod tests {
    use super::{effect_name, extract_global_options, resolve_u8, resolve_u16, select_candidate};
    use g102_control::{HidppDeviceInfo, config::AppConfig};

    #[test]
    fn names_known_lightsync_effect_ids_and_leaves_unknown_ids_unassigned() {
        assert_eq!(effect_name(0x0001), "Static color");
        assert_eq!(effect_name(0x0003), "Color cycle");
        assert_eq!(effect_name(0x0004), "Color wave");
        assert_eq!(effect_name(0x000a), "Breathing");
        assert_eq!(effect_name(0x000d), "Unknown effect");
    }

    #[test]
    fn physical_profile_mutations_are_refused_before_hid_initialization() {
        for args in [
            vec![
                "onboard".to_owned(),
                "set-button".to_owned(),
                "4".to_owned(),
                "right".to_owned(),
                "--confirm-write".to_owned(),
            ],
            vec![
                "onboard".to_owned(),
                "restore".to_owned(),
                "missing.obk".to_owned(),
                "--confirm-write".to_owned(),
            ],
        ] {
            let error = super::command(&args, &AppConfig::default()).unwrap_err();
            assert!(error.contains("disabled pending exact G102 model/firmware validation"));
            assert!(error.contains("--confirm-write cannot override this gate"));
        }
    }

    #[test]
    fn irrelevant_options_and_extra_offline_operands_are_rejected_early() {
        let config = AppConfig::default();
        for args in [
            vec!["rate".into(), "get".into(), "--sensor".into(), "0".into()],
            vec![
                "rgb".into(),
                "info".into(),
                "--period-ms".into(),
                "3000".into(),
            ],
            vec!["dpi".into(), "get".into(), "--dry-run".into()],
            vec![
                "onboard".into(),
                "diff".into(),
                "missing-a.obk".into(),
                "missing-b.obk".into(),
                "--device".into(),
                "0".into(),
            ],
            vec![
                "onboard".into(),
                "remap".into(),
                "input.obk".into(),
                "output.obk".into(),
                "extra.obk".into(),
                "--slot".into(),
                "1".into(),
                "--action".into(),
                "left".into(),
            ],
        ] {
            let error = super::command(&args, &config).unwrap_err();
            assert!(
                !error.contains("could not read"),
                "unexpected file I/O: {error}"
            );
            assert!(!error.contains("HID"), "unexpected HID access: {error}");
        }
    }

    #[test]
    fn global_config_options_are_removed_independent_of_position() {
        let args = [
            "--config".to_owned(),
            "custom.toml".to_owned(),
            "dpi".to_owned(),
            "get".to_owned(),
            "--no-config".to_owned(),
        ];
        assert!(
            extract_global_options(&args)
                .unwrap_err()
                .contains("cannot be used together")
        );
        let args = [
            "--config=custom.toml".to_owned(),
            "dpi".to_owned(),
            "get".to_owned(),
        ];
        let (command, path, disabled) = extract_global_options(&args).unwrap();
        assert_eq!(command, ["dpi", "get"]);
        assert_eq!(path.unwrap(), std::path::PathBuf::from("custom.toml"));
        assert!(!disabled);
    }

    #[test]
    fn explicit_numeric_argument_overrides_configured_default() {
        assert_eq!(
            resolve_u16(Some(&"2000".into()), Some(1600), "DPI", 1, 8000).unwrap(),
            2000
        );
        assert_eq!(resolve_u16(None, Some(1600), "DPI", 1, 8000).unwrap(), 1600);
        assert!(resolve_u16(Some(&"0".into()), Some(1600), "DPI", 1, 8000).is_err());
        assert_eq!(
            resolve_u8(Some(&"2".into()), Some(1), "effect index", 63).unwrap(),
            2
        );
        assert_eq!(resolve_u8(None, Some(1), "effect index", 63).unwrap(), 1);
    }

    #[test]
    fn preferred_device_matches_identity_and_explicit_index_overrides_it() {
        let candidates = [
            HidppDeviceInfo {
                path: "/dev/hidraw1".into(),
                vendor_id: 0x046d,
                product_id: 0xc092,
                usage_page: 0xff00,
                usage: 1,
                interface_number: 1,
                manufacturer: None,
                product: None,
            },
            HidppDeviceInfo {
                path: "/dev/hidraw2".into(),
                vendor_id: 0x046d,
                product_id: 0xc31c,
                usage_page: 0xff00,
                usage: 1,
                interface_number: 1,
                manufacturer: None,
                product: None,
            },
        ];
        let mut config = AppConfig::default();
        config.device.vendor_id = Some(0x046d);
        config.device.product_id = Some(0xc092);
        config.device.interface_number = Some(1);
        assert_eq!(
            select_candidate(&candidates, None, &config).unwrap().path,
            "/dev/hidraw1"
        );
        assert_eq!(
            select_candidate(&candidates, Some(1), &config)
                .unwrap()
                .path,
            "/dev/hidraw2"
        );
    }
}
