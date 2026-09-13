//! Socket/port collection for the process list.
//!
//! Parses `/proc/net/{tcp,tcp6,udp,udp6}` into an inode → port map and then
//! resolves those inodes through `/proc/<pid>/fd` so the process view can show
//! which process listens on which port.

use std::{collections::HashMap, fs};

use serde::{Deserialize, Serialize};

/// A single socket port owned by a process.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProcessPort {
    pub protocol: String,
    pub port: u16,
    /// TCP: socket is in LISTEN state. UDP: socket is bound.
    pub listening: bool,
}

/// Rules that drive which protocols are scanned and whether bound-only
/// sockets are reported. Populated from the caijuehub process rules.
#[derive(Debug, Clone)]
pub struct SocketScanConfig {
    pub protocols: Vec<String>,
    pub listen_only: bool,
}

impl Default for SocketScanConfig {
    fn default() -> Self {
        Self {
            protocols: vec![
                "tcp".to_owned(),
                "tcp6".to_owned(),
                "udp".to_owned(),
                "udp6".to_owned(),
            ],
            listen_only: false,
        }
    }
}

fn parse_port(hex: &str, protocol: &str) -> Option<ProcessPort> {
    let local = hex.rsplit(':').next()?;
    let port = u16::from_str_radix(local, 16).ok()?;

    let listening = if protocol.starts_with("udp") {
        // UDP sockets have no LISTEN state; a bound socket is what matters.
        true
    } else {
        // TCP_LISTEN == 0x0A
        false
    };

    Some(ProcessPort {
        protocol: protocol.to_owned(),
        port,
        listening,
    })
}

/// Builds the inode → port map by scanning the kernel socket tables once.
#[must_use]
pub fn collect(config: &SocketScanConfig) -> HashMap<u64, ProcessPort> {
    let mut map = HashMap::new();

    for protocol in &config.protocols {
        let path = format!("/proc/net/{protocol}");
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };

        for line in contents.lines().skip(1) {
            let tokens: Vec<&str> = line.split_whitespace().collect();

            let (Some(local), Some(state), Some(inode)) =
                (tokens.get(1), tokens.get(3), tokens.get(9))
            else {
                continue;
            };

            let Ok(inode) = inode.parse::<u64>() else {
                continue;
            };

            let mut port = match parse_port(local, protocol) {
                Some(port) => port,
                None => continue,
            };

            if protocol.starts_with("tcp") {
                port.listening = *state == "0A";
            }

            if config.listen_only && !port.listening {
                continue;
            }

            map.insert(inode, port);
        }
    }

    map
}

/// Resolves the sockets of a single process through `/proc/<pid>/fd`.
///
/// Sockets owned by other users may be unreadable without privileges; in that
/// case the process simply gets no ports.
#[must_use]
pub fn ports_for_pid(pid: libc::pid_t, map: &HashMap<u64, ProcessPort>) -> Vec<ProcessPort> {
    let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };

    let mut ports: Vec<ProcessPort> = Vec::new();

    for entry in entries.flatten() {
        let Ok(link) = fs::read_link(entry.path()) else {
            continue;
        };

        let target = link.to_string_lossy();
        let Some(rest) = target.strip_prefix("socket:[") else {
            continue;
        };
        let Some(inode) = rest.strip_suffix(']').and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let Some(port) = map.get(&inode) else {
            continue;
        };

        if !ports.contains(port) {
            ports.push(port.clone());
        }
    }

    ports.sort();
    ports
}
