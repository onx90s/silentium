use anyhow::{bail, Result};
use std::thread::sleep;
use std::time::{Duration, Instant};

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

/// Congela e retoma os processos do Discord em ciclos.
/// Não mexe em sockets, firewall ou outros programas.
pub fn jam(seconds: Option<u64>) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::jam(seconds)
    }
    #[cfg(windows)]
    {
        windows::jam(seconds)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = seconds;
        bail!("plataforma sem suporte a atrapalhar o Discord")
    }
}

trait JamControl {
    fn suspend(&mut self) -> Result<usize>;
    fn resume(&mut self);
}

fn run_jam<J: JamControl>(mut jammer: J, seconds: Option<u64>) -> Result<()> {
    crate::interrupt::install().map_err(anyhow::Error::msg)?;
    crate::interrupt::reset();

    println!("[+] atrapalhando só o Discord deste usuário — sem token, root ou admin");
    let deadline = seconds.map(|secs| Instant::now() + Duration::from_secs(secs));
    let mut first = true;
    while !crate::interrupt::hit() {
        if deadline.map(|end| Instant::now() >= end).unwrap_or(false) {
            break;
        }
        let count = jammer.suspend()?;
        if count == 0 {
            bail!("nenhum processo do Discord deste usuário para atrapalhar");
        }
        if first {
            println!("[+] {count} processo(s) do Discord em ciclos de 350ms/120ms");
            first = false;
        }
        jam_sleep(Duration::from_millis(350));
        jammer.resume();
        jam_sleep(Duration::from_millis(120));
    }
    jammer.resume();
    println!("[+] Discord restaurado");
    Ok(())
}

fn jam_sleep(total: Duration) {
    let end = Instant::now() + total;
    while Instant::now() < end && !crate::interrupt::hit() {
        let left = end.saturating_duration_since(Instant::now());
        sleep(Duration::from_millis(20).min(left));
    }
}

#[cfg(windows)]
mod windows {
    use super::{JamControl, ShapeSpec, POLICY_PREFIX, VOICE_PORT_END, VOICE_PORT_START};
    use anyhow::{bail, Context, Result};
    use std::collections::HashSet;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
    };

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

    struct ThreadJam {
        threads: Arc<Vec<windows_sys::Win32::Foundation::HANDLE>>,
        active: AtomicBool,
    }

    impl JamControl for ThreadJam {
        fn suspend(&mut self) -> Result<usize> {
            let mut count = 0;
            for handle in self.threads.iter() {
                if unsafe { SuspendThread(*handle) } != u32::MAX {
                    count += 1;
                }
            }
            if count > 0 {
                self.active.store(true, Ordering::SeqCst);
            }
            Ok(count)
        }

        fn resume(&mut self) {
            if self.active.swap(false, Ordering::SeqCst) {
                for handle in self.threads.iter() {
                    unsafe { ResumeThread(*handle) };
                }
            }
        }
    }

    impl Drop for ThreadJam {
        fn drop(&mut self) {
            self.resume();
            for handle in self.threads.iter() {
                unsafe { CloseHandle(*handle) };
            }
        }
    }

    pub fn jam(seconds: Option<u64>) -> Result<()> {
        let pids = crate::detect::discord_pids();
        if pids.is_empty() {
            bail!("Discord não está rodando neste usuário");
        }
        let allowed: HashSet<u32> = pids.into_iter().collect();
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            bail!("CreateToolhelp32Snapshot falhou");
        }
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut threads = Vec::new();
        let mut ok = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while ok {
            if allowed.contains(&entry.th32OwnerProcessID) {
                let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !handle.is_null() {
                    threads.push(handle);
                }
            }
            ok = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        if threads.is_empty() {
            bail!("não consegui abrir as threads do Discord");
        }
        let jammer = ThreadJam {
            threads: Arc::new(threads),
            active: AtomicBool::new(false),
        };
        super::run_jam(jammer, seconds)
    }

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
    use super::{JamControl, ShapeSpec, VOICE_PORT_END, VOICE_PORT_START};
    use anyhow::{bail, Context, Result};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const MARK: &str = "0x0d15";

    struct ProcessJam {
        pids: Arc<Vec<(u32, u64)>>,
        active: AtomicBool,
    }

    fn start_time(pid: u32) -> Option<u64> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, rest) = text.rsplit_once(')')?;
        rest.split_whitespace().nth(19)?.parse().ok()
    }

    impl JamControl for ProcessJam {
        fn suspend(&mut self) -> Result<usize> {
            let mut count = 0;
            for (pid, start) in self.pids.iter().copied() {
                if start_time(pid) != Some(start) {
                    continue;
                }
                if unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) } == 0 {
                    count += 1;
                }
            }
            if count > 0 {
                self.active.store(true, Ordering::SeqCst);
            }
            Ok(count)
        }

        fn resume(&mut self) {
            if self.active.swap(false, Ordering::SeqCst) {
                for (pid, start) in self.pids.iter().copied() {
                    if start_time(pid) == Some(start) {
                        unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) };
                    }
                }
            }
        }
    }

    impl Drop for ProcessJam {
        fn drop(&mut self) {
            self.resume();
        }
    }

    pub fn jam(seconds: Option<u64>) -> Result<()> {
        let pids: Vec<_> = crate::detect::discord_pids()
            .into_iter()
            .filter_map(|pid| start_time(pid).map(|start| (pid, start)))
            .collect();
        if pids.is_empty() {
            bail!("Discord não está rodando neste usuário");
        }
        let jammer = ProcessJam {
            pids: Arc::new(pids),
            active: AtomicBool::new(false),
        };
        super::run_jam(jammer, seconds)
    }

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
