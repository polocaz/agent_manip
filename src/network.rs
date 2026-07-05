//! Real network state for the daemon process, via `lsof -i -p <pid>`.
//!
//! The agent uploads to its master over a websocket; an ESTABLISHED TCP
//! connection from the daemon is the practical "uplink is connected" signal.

use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct Connection {
    pub protocol: String, // TCP / UDP
    pub local: String,
    pub remote: Option<String>, // None for listening/unbound sockets
    pub state: Option<String>,  // ESTABLISHED, LISTEN, ... (TCP only)
}

impl Connection {
    pub fn is_established(&self) -> bool {
        self.state.as_deref() == Some("ESTABLISHED")
    }
}

pub struct NetworkMonitor {
    connections: Vec<Connection>,
    last_error: Option<String>,
    check_interval: Duration,
    last_check: Option<Instant>,
}

impl NetworkMonitor {
    pub fn new() -> Self {
        Self {
            connections: Vec::new(),
            last_error: None,
            check_interval: Duration::from_secs(5),
            last_check: None,
        }
    }

    /// Refresh connection info for the daemon PID (rate-limited internally).
    pub fn update(&mut self, pid: Option<u32>) {
        if let Some(last) = self.last_check {
            if last.elapsed() < self.check_interval {
                return;
            }
        }
        self.last_check = Some(Instant::now());

        match pid {
            Some(pid) => match query_connections(pid) {
                Ok(conns) => {
                    self.connections = conns;
                    self.last_error = None;
                }
                Err(e) => {
                    self.connections.clear();
                    self.last_error = Some(e.to_string());
                }
            },
            None => {
                self.connections.clear();
                self.last_error = None;
            }
        }
    }

    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    pub fn established(&self) -> Vec<&Connection> {
        self.connections.iter().filter(|c| c.is_established()).collect()
    }

    pub fn is_connected(&self) -> bool {
        self.connections.iter().any(|c| c.is_established())
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

/// List the internet sockets held by `pid` using lsof (present on macOS by
/// default; commonly installed on Linux).
pub fn query_connections(pid: u32) -> Result<Vec<Connection>> {
    let output = Command::new("lsof")
        .args(["-nP", "-a", "-i", "-p", &pid.to_string()])
        .output()
        .map_err(|e| anyhow!("failed to run lsof: {}", e))?;

    // lsof exits 1 when there are simply no matching sockets; only treat it as
    // an error if it also printed to stderr.
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            return Err(anyhow!("lsof failed: {}", stderr.trim()));
        }
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_lsof_output(&stdout))
}

/// Parse `lsof -nP -i` output lines like:
/// `lsiagentd 123 root 12u IPv4 0x... 0t0 TCP 10.0.0.5:50000->1.2.3.4:443 (ESTABLISHED)`
fn parse_lsof_output(stdout: &str) -> Vec<Connection> {
    let mut conns = Vec::new();
    for line in stdout.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        // Find the protocol column; the columns before it can vary in width.
        let Some(proto_idx) = tokens.iter().position(|t| *t == "TCP" || *t == "UDP") else {
            continue;
        };
        let Some(name) = tokens.get(proto_idx + 1) else {
            continue;
        };
        let state = tokens
            .get(proto_idx + 2)
            .map(|s| s.trim_matches(&['(', ')'][..]).to_string());

        let (local, remote) = match name.split_once("->") {
            Some((l, r)) => (l.to_string(), Some(r.to_string())),
            None => (name.to_string(), None),
        };

        conns.push(Connection {
            protocol: tokens[proto_idx].to_string(),
            local,
            remote,
            state,
        });
    }
    conns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_established_and_listen_lines() {
        let out = "\
COMMAND     PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
lsiagentd  4242 root   12u  IPv4 0xabc123        0t0  TCP 10.0.0.5:50000->34.1.2.3:443 (ESTABLISHED)
lsiagentd  4242 root   13u  IPv4 0xabc124        0t0  TCP *:9090 (LISTEN)
lsiagentd  4242 root   14u  IPv4 0xabc125        0t0  UDP *:5353
";
        let conns = parse_lsof_output(out);
        assert_eq!(conns.len(), 3);

        assert_eq!(conns[0].protocol, "TCP");
        assert_eq!(conns[0].local, "10.0.0.5:50000");
        assert_eq!(conns[0].remote.as_deref(), Some("34.1.2.3:443"));
        assert!(conns[0].is_established());

        assert_eq!(conns[1].state.as_deref(), Some("LISTEN"));
        assert!(!conns[1].is_established());

        assert_eq!(conns[2].protocol, "UDP");
        assert_eq!(conns[2].remote, None);
    }
}
