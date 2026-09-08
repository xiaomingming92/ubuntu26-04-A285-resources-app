use anyhow::{Context, Result, bail};
use log::{debug, warn};
use serde::{Deserialize, Serialize};

use std::{
    io::{Read, Write},
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    caijuehub::strategies::sensor,
    config::LIBEXECDIR,
    utils::IS_FLATPAK,
};

/// Power metrics read from the AMD SMU through RyzenAdj.
///
/// All values are already in watts. They describe the whole APU SoC:
/// PPT FAST is the instantaneous power draw, PPT SLOW its short-time
/// average and STAPM the long-time (sustained) average. The corresponding
/// limits are the same numbers RyzenAdj reports as the current PPT/STAPM
/// limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AmdSmuMetrics {
    pub stapm_limit_w: Option<f64>,
    pub stapm_value_w: Option<f64>,
    pub ppt_fast_limit_w: Option<f64>,
    pub ppt_fast_value_w: Option<f64>,
    pub ppt_slow_limit_w: Option<f64>,
    pub ppt_slow_value_w: Option<f64>,
}

impl AmdSmuMetrics {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stapm_limit_w.is_none()
            && self.stapm_value_w.is_none()
            && self.ppt_fast_limit_w.is_none()
            && self.ppt_fast_value_w.is_none()
            && self.ppt_slow_limit_w.is_none()
            && self.ppt_slow_value_w.is_none()
    }

    /// Returns one of the metric names declared in `sensor-rules.toml`
    /// (e.g. `"ppt_fast_value"`, `"stapm_limit"`).
    #[must_use]
    pub fn metric(&self, metric: &str) -> Option<f64> {
        match metric {
            "stapm_limit" => self.stapm_limit_w,
            "stapm_value" => self.stapm_value_w,
            "ppt_fast_limit" => self.ppt_fast_limit_w,
            "ppt_fast_value" => self.ppt_fast_value_w,
            "ppt_slow_limit" => self.ppt_slow_limit_w,
            "ppt_slow_value" => self.ppt_slow_value_w,
            _ => None,
        }
    }
}

struct AmdSmuProcess {
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl AmdSmuProcess {
    fn request(&mut self) -> Result<AmdSmuMetrics> {
        trace_request();

        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;

        let mut len_bytes = [0_u8; (usize::BITS / 8) as usize];
        self.stdout.read_exact(&mut len_bytes)?;

        let len = usize::from_le_bytes(len_bytes);
        let mut output_bytes = vec![0; len];
        self.stdout.read_exact(&mut output_bytes)?;

        let metrics = rmp_serde::from_slice(&output_bytes)
            .context("unable to decode companion process output")?;

        Ok(metrics)
    }
}

fn trace_request() {
    debug!("Requesting AMD SMU power metrics from companion process…");
}

static SMU_PROCESS: LazyLock<Mutex<Option<AmdSmuProcess>>> =
    LazyLock::new(|| Mutex::new(None));

// Once the companion cannot be started (or dies), don't retry it for the
// remaining lifetime of the Resources process.
static SMU_COMPANION_DISABLED: AtomicBool = AtomicBool::new(false);

fn spawn_companion() -> Result<AmdSmuProcess> {
    if *IS_FLATPAK {
        bail!("AMD SMU companion process is not supported in Flatpak mode");
    }

    let helper_path = format!("{LIBEXECDIR}/{}", sensor::SMU_HELPER_NAME);
    debug!("Spawning AMD SMU companion process ({helper_path})…");

    let mut child = Command::new("pkexec")
        .args([
            "--disable-internal-agent",
            helper_path.as_str(),
            sensor::RYZENADJ_PATH,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("unable to spawn AMD SMU companion process")?;

    let stdin = child.stdin.take().context("no stdin for companion process")?;
    let stdout = child.stdout.take().context("no stdout for companion process")?;

    Ok(AmdSmuProcess { stdin, stdout })
}

/// Returns the current AMD SMU power metrics.
///
/// This returns an empty [`AmdSmuMetrics`] (instead of an error) whenever the
/// companion is unavailable, so callers can simply display “N/A”.
pub fn read_metrics() -> AmdSmuMetrics {
    if !sensor::SMU_ENABLED {
        return AmdSmuMetrics::default();
    }

    if SMU_COMPANION_DISABLED.load(Ordering::Relaxed) {
        return AmdSmuMetrics::default();
    }

    let mut guard = SMU_PROCESS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    if guard.is_none() {
        match spawn_companion() {
            Ok(process) => *guard = Some(process),
            Err(error) => {
                warn!("Unable to start AMD SMU companion process: {error}");
                SMU_COMPANION_DISABLED.store(true, Ordering::Relaxed);
                return AmdSmuMetrics::default();
            }
        }
    }

    let process = guard.as_mut().unwrap();

    match process.request() {
        Ok(metrics) => metrics,
        Err(error) => {
            warn!("AMD SMU companion process failed, disabling it: {error}");
            guard.take();
            SMU_COMPANION_DISABLED.store(true, Ordering::Relaxed);
            AmdSmuMetrics::default()
        }
    }
}
