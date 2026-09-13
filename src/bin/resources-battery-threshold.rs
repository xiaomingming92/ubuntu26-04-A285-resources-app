use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use resources::caijuehub::strategies::battery as strategy;

fn attribute_path(dir: &Path, primary: &str, fallback: &str) -> Result<PathBuf> {
    let primary_path = dir.join(primary);
    if primary_path.exists() {
        return Ok(primary_path);
    }

    let fallback_path = dir.join(fallback);
    if fallback_path.exists() {
        return Ok(fallback_path);
    }

    bail!(
        "neither {primary} nor {fallback} found in {}",
        dir.display()
    )
}

fn write_attribute(path: &Path, value: u32) -> Result<()> {
    fs::write(path, format!("{value}\n"))
        .with_context(|| format!("unable to write {value} to {}", path.display()))
}

fn set_thresholds(dir: &Path, start: u32, end: u32) -> Result<()> {
    if end > strategy::MAXIMUM {
        bail!(
            "thresholds must be within {}..={}",
            strategy::MINIMUM,
            strategy::MAXIMUM
        );
    }

    if start >= end {
        bail!("start threshold ({start}) must be lower than end threshold ({end})");
    }

    let start_path = attribute_path(
        dir,
        strategy::START_ATTRIBUTE,
        strategy::FALLBACK_START_ATTRIBUTE,
    )?;
    let end_path = attribute_path(
        dir,
        strategy::END_ATTRIBUTE,
        strategy::FALLBACK_END_ATTRIBUTE,
    )?;

    // Write the start threshold first: some ECs reject a start value that is
    // above the currently configured end threshold.
    write_attribute(&start_path, start)?;
    write_attribute(&end_path, end)?;

    // A plain sysfs write does not emit an event, so UPower (and therefore the
    // GNOME charging mode and the status icon) would keep showing stale values.
    // Nudging the power_supply uevent makes it re-read immediately.
    if strategy::NOTIFY_UPOWER {
        let uevent = dir.join("uevent");
        if uevent.exists() {
            let _ = fs::write(uevent, "change\n");
        }
    }

    println!("start={start} end={end}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.as_slice() {
        [command, dir, start, end] if command == "set" => {
            let start = start
                .parse::<u32>()
                .context("invalid start threshold")?;
            let end = end.parse::<u32>().context("invalid end threshold")?;
            set_thresholds(Path::new(dir), start, end)
        }
        _ => bail!("usage: resources-battery-threshold set <battery-sysfs-dir> <start> <end>"),
    }
}
