use anyhow::Result;

pub const VOICE_PORT_START: u16 = 50000;
pub const VOICE_PORT_END: u16 = 65535;

#[derive(Debug, Clone)]
pub struct Peer {
    pub ip: String,
    pub port: u16,
    pub pid: u32,
    pub process: String,
}

impl Peer {
    pub fn is_voice(&self) -> bool {
        (VOICE_PORT_START..=VOICE_PORT_END).contains(&self.port)
    }
}

impl Peer {
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }
}

/// PID do cliente Discord, se houver um rodando.
pub fn discord_running() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let entries = std::fs::read_dir("/proc").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_string_lossy().parse::<u32>().ok() else {
                continue;
            };
            let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
                continue;
            };
            if comm.to_ascii_lowercase().contains("discord") {
                return Some(pid);
            }
        }
        None
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-Process Discord,DiscordPTB,DiscordCanary,DiscordDevelopment \
                 -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Id",
            ])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

pub fn voice_peers() -> Result<Vec<Peer>> {
    #[cfg(windows)]
    {
        return windows::voice_peers();
    }
    #[cfg(target_os = "linux")]
    {
        linux::voice_peers()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("detecção local não implementada para esta plataforma")
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Peer;
    use anyhow::{bail, Context, Result};
    use std::process::Command;

    pub fn voice_peers() -> Result<Vec<Peer>> {
        let out = Command::new("ss")
            .args(["-unp"])
            .output()
            .context("ss: iproute2 instalado?")?;
        if !out.status.success() {
            bail!("ss -unp falhou");
        }
        Ok(parse(&String::from_utf8_lossy(&out.stdout)))
    }

    fn parse(text: &str) -> Vec<Peer> {
        let mut peers = Vec::new();
        for line in text.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 4 {
                continue;
            }
            let Some(owner) = cols.iter().find(|c| c.contains("users:((")) else {
                continue;
            };
            if !owner.to_ascii_lowercase().contains("discord") {
                continue;
            }
            let peer = cols[3];
            if peer.starts_with("*") || peer.starts_with("0.0.0.0") || peer.starts_with("[::]") {
                continue;
            }
            let (ip, port) = split_addr(peer);
            let pid = owner
                .split("pid=")
                .nth(1)
                .and_then(|rest| rest.split([',', ')']).next())
                .and_then(|digits| digits.parse::<u32>().ok())
                .unwrap_or(0);
            peers.push(Peer {
                ip,
                port,
                pid,
                process: "Discord".to_string(),
            });
        }
        peers.sort_by_key(|a| a.endpoint());
        peers.dedup_by(|a, b| a.endpoint() == b.endpoint());
        peers
    }

    fn split_addr(raw: &str) -> (String, u16) {
        let trimmed = raw.trim_start_matches('[');
        match trimmed.rsplit_once(':') {
            Some((host, port)) => (
                host.trim_end_matches(']').to_string(),
                port.parse().unwrap_or(0),
            ),
            None => (trimmed.to_string(), 0),
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::Peer;
    use anyhow::{bail, Context, Result};
    use std::process::Command;

    fn powershell(script: &str) -> Result<String> {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .context("não consegui executar powershell")?;
        if !out.status.success() {
            bail!(
                "powershell falhou: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub fn voice_peers() -> Result<Vec<Peer>> {
        let pids = powershell(
            "Get-Process Discord,DiscordPTB,DiscordCanary,DiscordDevelopment -ErrorAction SilentlyContinue | \
             ForEach-Object { $_.Id }",
        )?;
        let pids: Vec<u32> = pids
            .split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
            .collect();
        if pids.is_empty() {
            return Ok(Vec::new());
        }

        let table = powershell("netstat -ano -p UDP")?;
        Ok(parse(&table, &pids))
    }

    fn parse(table: &str, discord_pids: &[u32]) -> Vec<Peer> {
        let mut peers = Vec::new();
        for line in table.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 4 || !cols[0].eq_ignore_ascii_case("UDP") {
                continue;
            }
            let pid = cols[3].parse::<u32>().unwrap_or(0);
            if !discord_pids.contains(&pid) {
                continue;
            }
            let foreign = cols[2];
            if foreign.contains(":*")
                || foreign.starts_with("0.0.0.0")
                || foreign.starts_with('[') && foreign.contains("]:*")
            {
                continue;
            }
            let (ip, port) = match foreign.rsplit_once(':') {
                Some((host, port)) => (
                    host.trim_matches(|c| c == '[' || c == ']').to_string(),
                    port.parse().unwrap_or(0),
                ),
                None => continue,
            };
            if port == 0 {
                continue;
            }
            peers.push(Peer {
                ip,
                port,
                pid,
                process: "Discord".to_string(),
            });
        }
        peers.sort_by(|a, b| a.endpoint().cmp(&b.endpoint()));
        peers.dedup_by(|a, b| a.endpoint() == b.endpoint());
        peers
    }
}

#[cfg(test)]
mod tests {
    use super::Peer;

    fn peer(port: u16) -> Peer {
        Peer {
            ip: "162.159.129.232".to_string(),
            port,
            pid: 1,
            process: "Discord".to_string(),
        }
    }

    #[test]
    fn faixa_de_voz_e_reconhecida() {
        assert!(peer(50002).is_voice());
        assert!(peer(65535).is_voice());
        assert!(!peer(443).is_voice());
        assert!(!peer(49999).is_voice());
    }

    #[test]
    fn endpoint_formatado() {
        assert_eq!(peer(50002).endpoint(), "162.159.129.232:50002");
    }
}
