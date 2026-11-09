use std::process::ExitCode;

use g102_control::{
    connect,
    device::DeviceInspection,
    features::{dpi, onboard, report_rate, rgb},
    hid::HidapiTransport,
};
use hidapi::HidApi;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "help" || arg == "--help" || arg == "-h")
    {
        print_help();
        return ExitCode::SUCCESS;
    }
    let result = if args.is_empty() || args[0] == "inspect" {
        inspect()
    } else {
        command(&args)
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
        "Usage:\n  g102-control inspect\n  g102-control dpi get [--device N] [--sensor N]\n  g102-control dpi set <DPI> [--device N] [--sensor N]\n  g102-control rate get|list [--device N]\n  g102-control rate set <HZ> [--device N]\n  g102-control rgb info|effects [--device N]\n  g102-control rgb set <CLUSTER> <EFFECT_INDEX> <RRGGBB> [--period-ms N] [--brightness 1..100] [--direction left|right] [--device N]\n  g102-control onboard info|profiles [--device N]\n  g102-control onboard export <FILE> [--device N]\n  g102-control onboard decode <FILE>\n  g102-control onboard buttons <FILE>\n  g102-control onboard remap <INPUT> [OUTPUT] --slot <1..6> --action <ACTION> [--dry-run]\n  g102-control onboard diff <FILE_A> <FILE_B>\n  g102-control onboard set-button <SLOT> <ACTION> [--confirm-write] [--dry-run] [--device N] [--backup-file FILE] [--commit-timeout-ms MS]\n  g102-control onboard restore <BACKUP_FILE> [--confirm-write] [--dry-run] [--device N] [--backup-file FILE] [--commit-timeout-ms MS]\n\nActions: left, right, middle, back, forward, dpi-cycle, disabled.\nUse rgb effects to see zero-based cluster/effect indexes. Effect IDs 0x00 Off, 0x01 Static, 0x03 Cycle, 0x04 Wave, and 0x0a Breathing have known G102 HID++ layouts. Period defaults to 3000ms (allowed 1000..8000), brightness defaults to 100, and direction defaults right. Color is six-digit hex (e.g. ffffff); cycle/wave ignore it. Writes claim 0x8071 software control and apply to volatile RAM only; no onboard mode or persistent memory is changed. `onboard buttons`, `onboard decode`, `onboard remap`, and `onboard diff` operate strictly on local G102OBK1 backups. Physical `onboard set-button` and `onboard restore` writes are disabled pending exact G102 model/firmware validation; `--confirm-write` does not override this gate."
    );
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
            Ok(connected) => print_inspection(&connected.inspection),
            Err(error) => println!("  HID++ probe: {error}\n  No device settings were written."),
        }
    }
    Ok(())
}

fn command(args: &[String]) -> Result<(), String> {
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
    let mut sensor_index = 0u8;
    let mut effect_period_ms = 3000u16;
    let mut effect_brightness = 100u8;
    let mut wave_direction = rgb::WaveDirection::Right;
    let mut remap_slot = None;
    let mut remap_action = None;
    let mut dry_run = false;
    let mut backup_file = None;
    let mut commit_timeout_ms = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                dry_run = true;
            }
            "--confirm-write" => {}
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
                                .map_err(|_| "period must fit in 0..=65535 ms".to_owned())?
                        }
                        "--brightness" => {
                            effect_brightness = u8::try_from(parsed)
                                .map_err(|_| "brightness must fit in 0..=255".to_owned())?
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
        let expected_positional = if family == "rgb" && action == "set" {
            3
        } else if family == "onboard" && matches!(action, "export" | "decode" | "buttons") {
            1
        } else {
            usize::from(action == "set")
        };
        if positional.len() != expected_positional {
            return Err(if expected_positional == 1 {
                "set requires the expected value(s)"
            } else {
                "unexpected positional argument"
            }
            .into());
        }
    }

    let api = hid_api()?;
    let candidates: Vec<_> = HidapiTransport::enumerate(&api)
        .into_iter()
        .filter(|device| device.is_candidate())
        .collect();
    if candidates.is_empty() {
        return Err("no Logitech HID++ vendor collections found".into());
    }
    let selected = match device_index {
        Some(index) => candidates.get(index).ok_or_else(|| {
            format!(
                "device candidate {index} does not exist ({} candidate(s) found)",
                candidates.len()
            )
        })?,
        None if candidates.len() == 1 => &candidates[0],
        None => {
            return Err(format!(
                "{} candidates found; select one with --device N",
                candidates.len()
            ));
        }
    };
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
            let value: u16 = positional[0]
                .parse()
                .map_err(|_| "DPI must be an integer in 1..=65535".to_owned())?;
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
            let value: u16 = positional[0]
                .parse()
                .map_err(|_| "rate must be an integer Hz value".to_owned())?;
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
            let cluster: u8 = positional[0]
                .parse()
                .map_err(|_| "cluster must be an integer in 0..=255")?;
            let effect: u8 = positional[1]
                .parse()
                .map_err(|_| "effect index must be an integer in 0..=255")?;
            let color = parse_rgb(&positional[2])?;
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
                "Device acknowledged effect 0x{id:04x} for cluster {cluster}, effect index {effect} (volatile RAM). Read-back is not available; visible output and persistence are not verified."
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
        let verified_status = if button.action.is_verified_g102() {
            "Verified on G102"
        } else {
            "Unverified on G102"
        };
        println!(
            "    Slot {} [0x{:02x}..0x{:02x}]: {:02x?} -> {} [{}]",
            button.slot,
            offset,
            offset + 4,
            button.raw,
            button.action.display_description(),
            verified_status
        );
    }
    println!();
    println!("Evidence & Safety Constraints:");
    println!(
        "  - Stored slot ordering is sequential (slots 1..=6). Default assignments match standard physical"
    );
    println!(
        "    buttons, but physical button mapping is unverified without behavioral tracing on hardware."
    );
    println!(
        "  - Verified remappable targets: left, right, middle, back, forward, dpi-cycle, disable."
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
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("color must be six hexadecimal digits (RRGGBB)".into());
    }
    Ok([
        u8::from_str_radix(&hex[0..2], 16).unwrap(),
        u8::from_str_radix(&hex[2..4], 16).unwrap(),
        u8::from_str_radix(&hex[4..6], 16).unwrap(),
    ])
}

fn print_rgb(info: rgb::LightingInfo) {
    println!(
        "HID++ 0x8071 lighting: {} cluster(s); device NV capabilities 0x{:04x}, extended 0x{:04x}. Persistence not established.",
        info.cluster_count, info.nv_capabilities, info.extended_capabilities
    );
    println!(
        "  Software control flags 0x{:02x}; event flags 0x{:02x}",
        info.software_control.0, info.software_control.1
    );
    for cluster in info.clusters {
        println!(
            "  Cluster {}: location {} (0x{:04x}); display persistence 0x{:02x}; effect persistence {}; multi-LED pattern {}",
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
    use super::effect_name;

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
            let error = super::command(&args).unwrap_err();
            assert!(error.contains("disabled pending exact G102 model/firmware validation"));
            assert!(error.contains("--confirm-write cannot override this gate"));
        }
    }
}
