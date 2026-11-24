use anyhow::Result;

pub const VOICE_PORT_START: u16 = 50000;
pub const VOICE_PORT_END: u16 = 65535;

#[derive(Debug, Clone)]
pub struct Peer {
    pub ip: String,
    pub port: u16,
    pub pid: u32,
    pub process: String,
    /// Porta local do socket — no Windows é o sinal mais confiável.
    pub local_port: u16,
    /// Taxa medida em kbps nos dois sentidos (quando o sistema informa).
    pub rate_kbps: Option<u32>,
}

impl Peer {
    /// Call de voz = porta remota na faixa de mídia, OU porta local na faixa,
    /// OU tráfego contínuo suficiente (cobre mídia caindo no 443).
    pub fn is_voice(&self) -> bool {
        let faixa = VOICE_PORT_START..=VOICE_PORT_END;
        if faixa.contains(&self.port) {
            return true;
        }
        // No Windows o netstat esconde o peer: a porta local é o único sinal.
        #[cfg(windows)]
        if faixa.contains(&self.local_port) {
            return true;
        }
        self.rate_kbps.map(|rate| rate >= 20).unwrap_or(false)
    }

    pub fn confidence(&self) -> &'static str {
        let faixa = VOICE_PORT_START..=VOICE_PORT_END;
        if faixa.contains(&self.port) {
            "alta"
        } else if self.rate_kbps.unwrap_or(0) >= 20 {
            "média"
        } else {
            "baixa"
        }
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
    use std::collections::HashMap;
    use std::process::Command;
    use std::thread::sleep;
    use std::time::Duration;

    struct Raw {
        local: String,
        peer: String,
        local_port: u16,
        pid: u32,
        rx: u64,
        tx: u64,
    }

    pub fn voice_peers() -> Result<Vec<Peer>> {
        let first = snapshot()?;
        if first.is_empty() {
            return Ok(Vec::new());
        }
        sleep(Duration::from_millis(900));
        let second = snapshot()?;

        let antes: HashMap<(String, String), (u64, u64)> = first
            .iter()
            .map(|raw| ((raw.local.clone(), raw.peer.clone()), (raw.rx, raw.tx)))
            .collect();

        let mut peers = Vec::new();
        for raw in &second {
            let Some((rx0, tx0)) = antes.get(&(raw.local.clone(), raw.peer.clone())) else {
                continue;
            };
            let delta = (raw.rx.saturating_sub(*rx0) + raw.tx.saturating_sub(*tx0)) as f64 * 8.0;
            let kbps = (delta / 0.9 / 1000.0) as u32;
            let (ip, port) = split_addr(&raw.peer);
            peers.push(Peer {
                ip,
                port,
                pid: raw.pid,
                process: "Discord".to_string(),
                local_port: raw.local_port,
                rate_kbps: Some(kbps),
            });
        }
        peers.sort_by_key(|a| a.endpoint());
        peers.dedup_by(|a, b| a.endpoint() == b.endpoint());
        Ok(peers)
    }

    fn snapshot() -> Result<Vec<Raw>> {
        let out = Command::new("ss")
            .args(["-unpi"])
            .output()
            .context("ss: iproute2 instalado?")?;
        if !out.status.success() {
            bail!("ss -unpi falhou");
        }
        Ok(parse(&String::from_utf8_lossy(&out.stdout)))
    }

    fn parse(text: &str) -> Vec<Raw> {
        let mut out = Vec::new();
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
            if peer.starts_with('*') || peer.starts_with("[::]") || peer.starts_with("0.0.0.0") {
                continue;
            }
            let (_, local_port) = split_addr(cols[2]);
            let pid = owner
                .split("pid=")
                .nth(1)
                .and_then(|rest| rest.split([',', ')']).next())
                .and_then(|digits| digits.parse::<u32>().ok())
                .unwrap_or(0);
            out.push(Raw {
                local: cols[2].to_string(),
                peer: peer.to_string(),
                local_port,
                pid,
                rx: stat(line, "bytes_received:"),
                tx: stat(line, "bytes_sent:"),
            });
        }
        out
    }

    fn stat(line: &str, key: &str) -> u64 {
        line.split_whitespace()
            .find_map(|token| token.strip_prefix(key))
            .and_then(|value| value.trim_end_matches(',').parse::<u64>().ok())
            .unwrap_or(0)
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
            let (_, local_port) = match cols[1].rsplit_once(':') {
                Some((host, port)) => (host.to_string(), port.parse::<u16>().unwrap_or(0)),
                None => (String::new(), 0),
            };
            let foreign = cols[2];
            let sem_peer = foreign.contains(":*")
                || foreign.starts_with("0.0.0.0")
                || foreign.starts_with('[') && foreign.contains("]:*");
            if sem_peer {
                if local_port == 0 {
                    continue;
                }
                peers.push(Peer {
                    ip: String::new(),
                    port: 0,
                    pid,
                    process: "Discord".to_string(),
                    local_port,
                    rate_kbps: None,
                });
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
                local_port,
                rate_kbps: None,
            });
        }
        peers.sort_by_key(|a| a.endpoint());
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
            local_port: 0,
            rate_kbps: None,
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
    fn porta_local_so_conta_no_windows() {
        let mut p = peer(443);
        p.local_port = 50003;
        assert_eq!(p.is_voice(), cfg!(windows));
    }

    #[test]
    fn trafego_continuo_conta_como_call() {
        let mut p = peer(443);
        p.rate_kbps = Some(42);
        assert!(p.is_voice());
        p.rate_kbps = Some(2);
        assert!(!p.is_voice());
        p.rate_kbps = Some(15);
        assert!(!p.is_voice());
    }

    #[test]
    fn endpoint_formatado() {
        assert_eq!(peer(50002).endpoint(), "162.159.129.232:50002");
    }
}
