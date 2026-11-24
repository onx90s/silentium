use anyhow::Result;

pub const VOICE_PORT_START: u16 = 50000;
pub const VOICE_PORT_END: u16 = 65535;
#[allow(dead_code)]
pub const POLICY_PREFIX: &str = "Silentium";

#[derive(Debug, Clone)]
pub struct ShapeSpec {
    pub rate_kbps: u64,
    pub loss_percent: Option<u8>,
    pub delay_ms: Option<u32>,
    #[allow(dead_code)]
    pub iface: Option<String>,
}

pub fn apply(spec: &ShapeSpec) -> Result<()> {
    #[cfg(windows)]
    {
        return windows::apply(spec);
    }
    #[cfg(target_os = "linux")]
    {
        linux::apply(spec)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = spec;
        anyhow::bail!("plataforma sem suporte a shaping nesta versão")
    }
}

pub fn clear(iface: Option<String>) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = iface;
        return windows::clear();
    }
    #[cfg(target_os = "linux")]
    {
        linux::clear(iface)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = iface;
        anyhow::bail!("plataforma sem suporte a shaping nesta versão")
    }
}

#[cfg(windows)]
mod windows {
    use super::{ShapeSpec, POLICY_PREFIX, VOICE_PORT_END, VOICE_PORT_START};
    use anyhow::{bail, Context, Result};
    use std::process::Command;

    fn powershell(script: &str) -> Result<()> {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .context("não consegui executar powershell")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let std_out = String::from_utf8_lossy(&out.stdout).trim().to_string();
            bail!("powershell falhou: {} {}", err, std_out);
        }
        Ok(())
    }

    const BLOCK_RULE: &str = "Silentium-block";

    pub fn block(on: bool) -> Result<()> {
        if !on {
            let script = format!(
                "Get-NetFirewallRule -DisplayName '{BLOCK_RULE}-*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule"
            );
            powershell(&script)?;
            println!("[+] bloqueio do Discord removido");
            return Ok(());
        }

        let script = r#"
$ErrorActionPreference = 'Stop'
$paths = New-Object System.Collections.Generic.List[string]
Get-Process Discord, DiscordPTB, DiscordCanary, DiscordDevelopment -ErrorAction SilentlyContinue |
    ForEach-Object { if ($_.Path) { $paths.Add($_.Path) } }
$roots = @(
    (Join-Path $env:LOCALAPPDATA 'Discord'),
    (Join-Path $env:LOCALAPPDATA 'DiscordPTB'),
    (Join-Path $env:LOCALAPPDATA 'DiscordCanary'),
    (Join-Path $env:PROGRAMFILES 'Discord'),
    (Join-Path ${env:PROGRAMFILES(x86)} 'Discord')
)
foreach ($root in $roots) {
    if ($root -and (Test-Path $root)) {
        Get-ChildItem -Path $root -Recurse -Filter 'Discord.exe' -ErrorAction SilentlyContinue |
            ForEach-Object { $paths.Add($_.FullName) }
    }
}
$paths = $paths | Sort-Object -Unique
if ($paths.Count -eq 0) { Write-Error 'Discord.exe não encontrado'; exit 2 }
Get-NetFirewallRule -DisplayName 'SILENTIUM_RULE-*' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
$i = 0
foreach ($p in $paths) {
    New-NetFirewallRule -DisplayName ("SILENTIUM_RULE-{0}" -f $i) -Direction Outbound `
        -Program $p -Action Block -Profile Any -Enabled True | Out-Null
    $i++
}
Write-Output ("regras: {0}" -f $paths.Count)
"#;
        let script = script.replace("SILENTIUM_RULE", BLOCK_RULE);
        powershell(&script).context("aplicando regras de firewall (rode como Administrador)")?;
        println!("[+] internet do Discord cortada — só os processos dele, resto do PC intacto");
        Ok(())
    }

    pub fn apply(spec: &ShapeSpec) -> Result<()> {
        if spec.loss_percent.is_some() || spec.delay_ms.is_some() {
            eprintln!(
                "[!] no Windows a pilha nativa só limita banda; --loss/--delay são ignorados aqui"
            );
        }
        let bits = spec.rate_kbps.saturating_mul(1000).max(1000);

        for (name, port) in [
            (format!("{POLICY_PREFIX}-out"), "IPDstPort"),
            (format!("{POLICY_PREFIX}-in"), "IPSrcPort"),
        ] {
            let script = format!(
                "New-NetQosPolicy -Name '{name}' -PolicyStore ActiveStore \
                 -IPProtocolMatchCondition UDP \
                 -{port}StartMatchCondition {VOICE_PORT_START} \
                 -{port}EndMatchCondition {VOICE_PORT_END} \
                 -ThrottleRateActionBitsPerSecond {bits} -ErrorAction Stop"
            );
            powershell(&script)
                .with_context(|| format!("aplicando política {name} (rode como Administrador)"))?;
        }

        println!(
            "[+] QoS ativa: UDP {VOICE_PORT_START}-{VOICE_PORT_END} limitado a {} kbps (envio e recebimento)",
            spec.rate_kbps
        );
        Ok(())
    }

    pub fn block_endpoint(endpoint: &str) -> Result<()> {
        let ip = endpoint
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(endpoint);
        let script = format!(
            "New-NetFirewallRule -DisplayName '{POLICY_PREFIX}-media' -Direction Outbound \
             -RemoteAddress {ip} -Action Block -Profile Any -ErrorAction Stop | Out-Null"
        );
        powershell(&script).context("regra de firewall por endereço (Administrador)")?;
        println!("[+] mídia de voz cortada para {ip}");
        Ok(())
    }

    pub fn clear_endpoint() -> Result<()> {
        let script = format!(
            "Get-NetFirewallRule -DisplayName '{POLICY_PREFIX}-media' -ErrorAction SilentlyContinue | Remove-NetFirewallRule"
        );
        powershell(&script)?;
        println!("[+] corte por endereço removido");
        Ok(())
    }

    pub fn clear() -> Result<()> {
        for name in [
            format!("{POLICY_PREFIX}-out"),
            format!("{POLICY_PREFIX}-in"),
        ] {
            let script = format!(
                "Remove-NetQosPolicy -Name '{name}' -PolicyStore ActiveStore -Confirm:$false -ErrorAction SilentlyContinue"
            );
            powershell(&script)?;
        }
        println!("[+] políticas {POLICY_PREFIX}-* removidas");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ShapeSpec, VOICE_PORT_END, VOICE_PORT_START};
    use anyhow::{bail, Context, Result};
    use std::process::Command;

    const MARK: &str = "0x0d15";

    pub fn run(program: &str, args: &[&str]) -> Result<()> {
        let out = Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("executando {program}"))?;
        if !out.status.success() {
            bail!(
                "{program} {} falhou: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    fn detect_iface() -> Result<String> {
        let out = Command::new("ip")
            .args(["route", "get", "1.1.1.1"])
            .output()
            .context("ip route: iproute2 instalado?")?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let mut words = text.split_whitespace();
        while let Some(word) = words.next() {
            if word == "dev" {
                if let Some(iface) = words.next() {
                    return Ok(iface.to_string());
                }
            }
        }
        bail!("não consegui detectar a interface de saída");
    }

    fn ports() -> String {
        format!("{VOICE_PORT_START}:{VOICE_PORT_END}")
    }

    fn ensure_mark(dir: &str) -> Result<()> {
        let ports = ports();
        let check = [
            "-t",
            "mangle",
            "-C",
            "OUTPUT",
            "-p",
            "udp",
            dir,
            &ports,
            "-j",
            "MARK",
            "--set-mark",
            MARK,
        ];
        if run("iptables", &check).is_ok() {
            return Ok(());
        }
        let add = [
            "-t",
            "mangle",
            "-A",
            "OUTPUT",
            "-p",
            "udp",
            dir,
            &ports,
            "-j",
            "MARK",
            "--set-mark",
            MARK,
        ];
        run("iptables", &add)
    }

    pub fn block() -> Result<()> {
        let ports = ports();
        for dir in ["--dport", "--sport"] {
            run(
                "iptables",
                &["-C", "OUTPUT", "-p", "udp", dir, &ports, "-j", "DROP"],
            )
            .or_else(|_| {
                run(
                    "iptables",
                    &["-A", "OUTPUT", "-p", "udp", dir, &ports, "-j", "DROP"],
                )
            })?;
        }
        println!("[+] UDP de voz bloqueado (Linux não isola por processo; uso fica na faixa 50000-65535)");
        Ok(())
    }

    pub fn unblock() -> Result<()> {
        let ports = ports();
        for dir in ["--dport", "--sport"] {
            let _ = run(
                "iptables",
                &["-D", "OUTPUT", "-p", "udp", dir, &ports, "-j", "DROP"],
            );
        }
        println!("[+] bloqueio removido");
        Ok(())
    }

    pub fn apply(spec: &ShapeSpec) -> Result<()> {
        let iface = match &spec.iface {
            Some(name) => name.clone(),
            None => detect_iface()?,
        };
        ensure_mark("--dport")?;
        ensure_mark("--sport")?;

        let mut netem = format!("rate {}kbit", spec.rate_kbps.max(1));
        if let Some(loss) = spec.loss_percent {
            netem.push_str(&format!(" loss {loss}%"));
        }
        if let Some(delay) = spec.delay_ms {
            netem.push_str(&format!(" delay {delay}ms 200ms distribution normal"));
        }

        let _ = run("tc", &["qdisc", "del", "dev", &iface, "root"]);
        run(
            "tc",
            &[
                "qdisc", "add", "dev", &iface, "root", "handle", "1:", "prio",
            ],
        )?;
        run(
            "tc",
            &[
                "qdisc", "add", "dev", &iface, "parent", "1:3", "handle", "30:", "netem", &netem,
            ],
        )?;
        run(
            "tc",
            &[
                "filter", "add", "dev", &iface, "parent", "1:0", "handle", MARK, "fw", "flowid",
                "1:3",
            ],
        )?;

        println!("[+] netem em {iface}: {netem}");
        Ok(())
    }

    pub fn clear(iface: Option<String>) -> Result<()> {
        let iface = match iface {
            Some(name) => name,
            None => detect_iface()?,
        };
        let ports = ports();
        let _ = run("tc", &["qdisc", "del", "dev", &iface, "root"]);
        for dir in ["--dport", "--sport"] {
            let _ = run(
                "iptables",
                &[
                    "-t",
                    "mangle",
                    "-D",
                    "OUTPUT",
                    "-p",
                    "udp",
                    dir,
                    &ports,
                    "-j",
                    "MARK",
                    "--set-mark",
                    MARK,
                ],
            );
        }
        println!("[+] shaping removido de {iface}");
        Ok(())
    }
}

pub fn block() -> Result<()> {
    #[cfg(windows)]
    {
        return windows::block(true);
    }
    #[cfg(target_os = "linux")]
    {
        linux::block()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("plataforma sem suporte a bloqueio por processo")
    }
}

pub fn unblock() -> Result<()> {
    #[cfg(windows)]
    {
        return windows::block(false);
    }
    #[cfg(target_os = "linux")]
    {
        linux::unblock()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("plataforma sem suporte a bloqueio por processo")
    }
}

/// Corta só o fluxo de mídia com o servidor informado ("ip:porta").
pub fn block_endpoint(endpoint: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let (ip, port) = endpoint.rsplit_once(':').unwrap_or((endpoint, "0"));
        linux::run(
            "iptables",
            &[
                "-C", "OUTPUT", "-p", "udp", "-d", ip, "--dport", port, "-j", "DROP",
            ],
        )
        .or_else(|_| {
            linux::run(
                "iptables",
                &[
                    "-A", "OUTPUT", "-p", "udp", "-d", ip, "--dport", port, "-j", "DROP",
                ],
            )
        })?;
        println!("[+] mídia de voz cortada para {endpoint}");
        Ok(())
    }
    #[cfg(windows)]
    {
        windows::block_endpoint(endpoint)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = endpoint;
        anyhow::bail!("sem suporte a corte por endpoint")
    }
}

/// Desfaz o corte por endpoint.
pub fn unblock_endpoint(endpoint: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let (ip, port) = endpoint.rsplit_once(':').unwrap_or((endpoint, "0"));
        let _ = linux::run(
            "iptables",
            &[
                "-D", "OUTPUT", "-p", "udp", "-d", ip, "--dport", port, "-j", "DROP",
            ],
        );
        println!("[+] corte removido de {endpoint}");
        Ok(())
    }
    #[cfg(windows)]
    {
        windows::clear_endpoint()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = endpoint;
        anyhow::bail!("sem suporte a corte por endpoint")
    }
}
