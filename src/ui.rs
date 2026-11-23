// rust — src/ui.rs — tela inicial: banner, diagnóstico local e menu interativo
use crate::cli::{Cli, TargetArgs, VoiceCmd};
use crate::detect;
use anyhow::Result;
use std::io::{self, IsTerminal, Write};
use std::process::Command;

const ACCENT: &str = "\x1b[38;5;111m";
const OK: &str = "\x1b[38;5;114m";
const WARN: &str = "\x1b[38;5;179m";
const BAD: &str = "\x1b[38;5;203m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const OFF: &str = "\x1b[0m";

const BANNER: [&str; 6] = [
    "██████╗  ██╗ ██╗     ██████╗ ███╗   ███╗ ████████╗ ██╗ ██╗   ██╗ ███╗   ███╗",
    "██╔═══╝  ██║ ██║     ██╔═══╝ ████╗ ████║ ╚══██╔══╝ ██║ ██║   ██║ ████╗ ████║",
    "██████╗  ██║ ██║     █████╗  ██╔████╔██║    ██║    ██║ ██║   ██║ ██╔████╔██║",
    "╚═══██║  ██║ ██║     ██╔══╝  ██║╚██╔╝██║    ██║    ██║ ██║   ██║ ██║╚██╔╝██║",
    "██████║  ██║ ██████╗ ██████╗ ██║ ╚═╝ ██║    ██║    ██║ ╚██████╔╝ ██║ ╚═╝ ██║",
    "╚═════╝  ╚═╝ ╚═════╝ ╚═════╝ ╚═╝     ╚═╝    ╚═╝    ╚═╝  ╚═════╝  ╚═╝     ╚═╝",
];

const MENU: [(&str, &str, &str); 7] = [
    ("1", "detectar call", "sockets locais, sem token"),
    ("2", "ver quem está em voz", "precisa de token"),
    ("3", "derrubar todos da call", "precisa de token"),
    ("4", "mutar todos", "precisa de token"),
    ("5", "ensurdecer todos", "precisa de token"),
    ("6", "restaurar todos", "desfaz mute e deafen"),
    ("7", "cortar internet do Discord", "precisa de root"),
];

pub fn interactive(cli: &Cli) -> Result<()> {
    let color = io::stdout().is_terminal();
    banner(color);
    status(cli, color);

    if !io::stdin().is_terminal() {
        println!(
            "\n{}sem terminal interativo — use silentium --help para todos os comandos{}",
            dim(color),
            reset(color)
        );
        return Ok(());
    }

    menu(color);
    loop {
        print!("\n{}silentium ›{} ", accent(color), reset(color));
        io::stdout().flush()?;

        let Some(line) = read_line()? else {
            break;
        };
        let choice = line.trim().to_ascii_lowercase();
        if choice.is_empty() {
            continue;
        }
        if matches!(choice.as_str(), "q" | "quit" | "sair" | "exit") {
            println!("{}até.{}", dim(color), reset(color));
            break;
        }

        println!();
        match choice.as_str() {
            "1" => {
                crate::print_detect()?;
            }
            "2" => voice(cli, VoiceCmd::List(TargetArgs::default()))?,
            "3" => voice(cli, VoiceCmd::Drop(TargetArgs::default()))?,
            "4" => voice(cli, VoiceCmd::Mute(TargetArgs::default()))?,
            "5" => voice(cli, VoiceCmd::Deafen(TargetArgs::default()))?,
            "6" => voice(cli, VoiceCmd::Restore(TargetArgs::default()))?,
            "7" => cut_discord(color)?,
            "?" | "h" | "help" => help_hint(color),
            other => println!(
                "{}opção desconhecida: {}{}",
                bad(color),
                other,
                reset(color)
            ),
        }
        menu(color);
    }
    Ok(())
}

fn banner(color: bool) {
    println!();
    for line in BANNER {
        println!("  {}{}{}", accent(color), line, reset(color));
    }
    println!(
        "  {}kill switch de voz para Discord · v{}{}",
        bold(color),
        env!("CARGO_PKG_VERSION"),
        reset(color)
    );
}

fn status(cli: &Cli, color: bool) {
    println!();
    row(color, "sistema", &platform_line());
    row(color, "discord", &discord_line(color));
    row(color, "call", &call_line(color));
    row(color, "token", &token_line(cli, color));
    row(color, "guild", &guild_line(cli, color));
    row(color, "firewall", &privilege_line(color));
}

fn row(color: bool, label: &str, value: &str) {
    println!("  {}{:<9}{}  {}", dim(color), label, reset(color), value);
}

fn dot(color: bool, on: bool) -> String {
    if on {
        format!("{}{}●{}", ok(color), "", reset(color))
    } else {
        format!("{}○{}", dim(color), reset(color))
    }
}

fn platform_line() -> String {
    let arch = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    #[cfg(unix)]
    {
        let uid = Command::new("id").arg("-u").output().ok().and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        });
        match uid {
            Some(0) => format!("{arch} · uid 0 (root)"),
            Some(uid) => format!("{arch} · uid {uid}"),
            None => arch,
        }
    }
    #[cfg(not(unix))]
    {
        arch
    }
}

fn discord_line(color: bool) -> String {
    match detect::discord_running() {
        Some(pid) => format!("{} rodando — pid {pid}", dot(color, true)),
        None => format!("{} não encontrado", dot(color, false)),
    }
}

fn call_line(color: bool) -> String {
    match detect::voice_peers() {
        Ok(peers) => {
            let voice: Vec<_> = peers.iter().filter(|p| p.is_voice()).collect();
            if voice.is_empty() {
                format!("{} nenhuma call ativa", dot(color, false))
            } else {
                let ports: Vec<String> = voice.iter().map(|p| p.port.to_string()).take(4).collect();
                format!(
                    "{} ativa — {} socket(s) de mídia ({}{})",
                    dot(color, true),
                    voice.len(),
                    ports.join(", "),
                    if voice.len() > 4 { ", …" } else { "" }
                )
            }
        }
        Err(_) => format!("{} não verificável (ss indisponível)", dot(color, false)),
    }
}

fn token_line(cli: &Cli, color: bool) -> String {
    match &cli.token {
        Some(t) => format!(
            "{} definido ({}…{})",
            dot(color, true),
            t.chars().take(4).collect::<String>(),
            if cli.user_token { ", user token" } else { "" }
        ),
        None => format!("{} DISCORD_TOKEN não definido", dot(color, false)),
    }
}

fn guild_line(cli: &Cli, color: bool) -> String {
    match &cli.guild {
        Some(g) => format!("{} {g}", dot(color, true)),
        None => format!("{} --guild não definido (use --invite)", dot(color, false)),
    }
}

fn privilege_line(color: bool) -> String {
    if is_root() {
        format!(
            "{} privilegiado — pode cortar e moldar tráfego",
            dot(color, true)
        )
    } else {
        format!("{} sem privilégio — net só com sudo", dot(color, false))
    }
}

fn is_root() -> bool {
    #[cfg(unix)]
    {
        Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn menu(color: bool) {
    println!("\n  {}menu{}", bold(color), reset(color));
    for (key, label, hint) in MENU {
        println!(
            "  {}{:>2}{}  {:<28} {}{}{}",
            accent(color),
            key,
            reset(color),
            label,
            dim(color),
            hint,
            reset(color)
        );
    }
    println!(
        "  {}{:>2}{}  {:<28} {}todos os comandos e flags{}",
        dim(color),
        "?",
        reset(color),
        "ajuda completa",
        dim(color),
        reset(color)
    );
    println!(
        "  {}{:>2}{}  {:<28} {}encerra o menu{}",
        dim(color),
        "q",
        reset(color),
        "sair",
        dim(color),
        reset(color)
    );
}

fn help_hint(color: bool) {
    println!(
        "{}silentium --help{}          lista comandos",
        dim(color),
        reset(color)
    );
    println!(
        "{}silentium voice list --invite discord.gg/xxx{}",
        dim(color),
        reset(color)
    );
    println!(
        "{}silentium net auto --seconds 30{}",
        dim(color),
        reset(color)
    );
}

fn voice(cli: &Cli, cmd: VoiceCmd) -> Result<()> {
    if cli.token.is_none() {
        println!(
            "{}token ausente — exporte DISCORD_TOKEN ou use --token{}",
            warn(color_or_off()),
            reset(color_or_off())
        );
        return Ok(());
    }
    crate::voice_dispatch(cli, &cmd)
}

fn cut_discord(color: bool) -> Result<()> {
    if !is_root() {
        println!(
            "{}sem privilégio.{} isso mexe no firewall — rode com sudo: {}sudo silentium{}",
            warn(color),
            reset(color),
            bold(color),
            reset(color)
        );
        return Ok(());
    }
    println!(
        "{}cortando o Discord por processo…{}",
        dim(color),
        reset(color)
    );
    crate::net::block()?;
    println!(
        "{}internet do Discord cortada.{} Enter restaura.",
        warn(color),
        reset(color)
    );
    let _ = read_line()?;
    crate::net::unblock()?;
    Ok(())
}

fn read_line() -> Result<Option<String>> {
    let mut buf = String::new();
    let read = io::stdin().read_line(&mut buf)?;
    if read == 0 {
        return Ok(None);
    }
    Ok(Some(buf))
}

fn color_or_off() -> bool {
    io::stdout().is_terminal()
}

fn accent(on: bool) -> &'static str {
    if on {
        ACCENT
    } else {
        ""
    }
}
fn ok(on: bool) -> &'static str {
    if on {
        OK
    } else {
        ""
    }
}
fn warn(on: bool) -> &'static str {
    if on {
        WARN
    } else {
        ""
    }
}
fn bad(on: bool) -> &'static str {
    if on {
        BAD
    } else {
        ""
    }
}
fn dim(on: bool) -> &'static str {
    if on {
        DIM
    } else {
        ""
    }
}
fn bold(on: bool) -> &'static str {
    if on {
        BOLD
    } else {
        ""
    }
}
fn reset(on: bool) -> &'static str {
    if on {
        OFF
    } else {
        ""
    }
}
