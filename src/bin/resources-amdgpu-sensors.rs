use std::{
    env,
    io::{Read, Write},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};
use resources::{
    caijuehub::{
        smu::AmdSmuMetrics,
        strategies::sensor::RYZENADJ_PATH as DEFAULT_RYZENADJ,
    },
};

fn parse_ryzenadj_info(stdout: &str) -> AmdSmuMetrics {
    let mut metrics = AmdSmuMetrics::default();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }

        let cells: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if cells.len() < 3 {
            continue;
        }

        let name = cells[1];
        let value = cells[2].parse::<f64>().ok();

        match name {
            "STAPM LIMIT" => metrics.stapm_limit_w = value,
            "STAPM VALUE" => metrics.stapm_value_w = value,
            "PPT LIMIT FAST" => metrics.ppt_fast_limit_w = value,
            "PPT VALUE FAST" => metrics.ppt_fast_value_w = value,
            "PPT LIMIT SLOW" => metrics.ppt_slow_limit_w = value,
            "PPT VALUE SLOW" => metrics.ppt_slow_value_w = value,
            _ => {}
        }
    }

    metrics
}

fn fetch_metrics(ryzenadj_path: &str) -> Result<AmdSmuMetrics> {
    let output = Command::new(ryzenadj_path)
        .arg("--info")
        .output()
        .context("unable to run RyzenAdj")?;

    if !output.status.success() {
        bail!(
            "RyzenAdj exited with {}",
            output
                .status
                .code()
                .map_or_else(|| "unknown status".into(), |code| code.to_string())
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_ryzenadj_info(&stdout))
}

fn send_metrics(metrics: &AmdSmuMetrics, stdout: &mut impl Write) -> Result<()> {
    let encoded = rmp_serde::to_vec(metrics).context("unable to encode AMD SMU metrics")?;
    let len_byte_array = encoded.len().to_le_bytes();

    stdout.write_all(&len_byte_array)?;
    stdout.write_all(&encoded)?;
    stdout.flush()?;

    Ok(())
}

fn send_metrics_text(metrics: &AmdSmuMetrics, stdout: &mut impl Write) -> Result<()> {
    writeln!(stdout, "STAPM LIMIT = {}", power_text(metrics.stapm_limit_w))?;
    writeln!(stdout, "STAPM VALUE = {}", power_text(metrics.stapm_value_w))?;
    writeln!(stdout, "PPT LIMIT FAST = {}", power_text(metrics.ppt_fast_limit_w))?;
    writeln!(stdout, "PPT VALUE FAST = {}", power_text(metrics.ppt_fast_value_w))?;
    writeln!(stdout, "PPT LIMIT SLOW = {}", power_text(metrics.ppt_slow_limit_w))?;
    writeln!(stdout, "PPT VALUE SLOW = {}", power_text(metrics.ppt_slow_value_w))?;
    stdout.flush()?;
    Ok(())
}

fn power_text(value: Option<f64>) -> String {
    value.map_or_else(|| "N/A".to_string(), |value| format!("{value:.3} W"))
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);

    let ryzenadj_path = match args.next() {
        Some(path) => path,
        None => DEFAULT_RYZENADJ.to_string(),
    };

    let mut once = false;
    let mut text = false;
    for arg in args {
        match arg.as_str() {
            "--once" => once = true,
            "--text" => text = true,
            _ => {}
        }
    }

    if !Path::new(&ryzenadj_path).exists() {
        eprintln!("RyzenAdj not found at {ryzenadj_path}");
        std::process::exit(1);
    }

    let mut stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    let mut stdout = std::io::BufWriter::new(stdout);

    loop {
        if once {
            let metrics = fetch_metrics(&ryzenadj_path).unwrap_or_default();
            if text {
                send_metrics_text(&metrics, &mut stdout)?;
            } else {
                send_metrics(&metrics, &mut stdout)?;
            }
            break;
        }

        let mut buffer = [0; 1];
        if stdin.read_exact(&mut buffer).is_err() {
            // Resources closed our stdin, so it is time to exit.
            break;
        }

        // Request-driven: Resources asks exactly once per UI refresh, so the
        // SMU polling cadence follows Resources' refresh speed.
        let metrics = match fetch_metrics(&ryzenadj_path) {
            Ok(metrics) => metrics,
            Err(error) => {
                eprintln!("RyzenAdj failed: {error}");
                AmdSmuMetrics::default()
            }
        };

        send_metrics(&metrics, &mut stdout)?;
    }

    Ok(())
}
