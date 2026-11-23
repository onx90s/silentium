mod cli;
mod config;
mod detect;
mod discord;
mod gateway;
mod net;
mod ui;
mod update;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::{Cli, Cmd, NetCmd, TargetArgs, VoiceCmd};
use discord::Api;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

struct Target {
    user_id: String,
    name: String,
    channel_id: String,
    mute: bool,
    deaf: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.clone() {
        None => ui::interactive(&cli),
        Some(Cmd::Net(cmd)) => net_dispatch(&cmd),
        Some(Cmd::Whoami) => {
            let api = build_api(&cli)?;
            let me = api.me()?;
            println!("bot: {} ({})", me.username, me.id);
            Ok(())
        }
        Some(Cmd::Detect) => print_detect(),
        Some(Cmd::Resolve { invite }) => {
            let api = build_api(&cli)?;
            let resolved = api.resolve_invite(&invite)?;
            match (resolved.guild, resolved.channel) {
                (Some(guild), Some(channel)) => println!(
                    "guild={}  nome={:?}\ncanal={}  nome={:?}",
                    guild.id, guild.name, channel.id, channel.name
                ),
                (Some(guild), None) => println!("guild={}  nome={:?}", guild.id, guild.name),
                _ => println!("convite não aponta para servidor (DM ou inválido)"),
            }
            Ok(())
        }
        Some(Cmd::Update { check }) => update::run(check),
        Some(Cmd::Voice(cmd)) => voice_dispatch(&cli, &cmd),
    }
}

/// Imprime o estado da call olhando só os sockets locais do Discord.
/// Lista quem está em voz usando o gateway (aceita token de usuário).
pub(crate) fn who(cli: &Cli) -> Result<()> {
    let token = token_of(cli).context("sem token")?;
    let snap = gateway::snapshot(&token, uses_user_token(cli), 6)?;
    if snap.users.is_empty() {
        println!("ninguém em voz nas suas calls");
        return Ok(());
    }
    let mut atual = String::new();
    for user in &snap.users {
        if user.guild_id != atual {
            let nome = snap
                .guilds
                .iter()
                .find(|(id, _)| id == &user.guild_id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| user.guild_id.clone());
            println!("\n{nome}  ({})", user.guild_id);
            atual = user.guild_id.clone();
        }
        let marca = if user.user_id == snap.me {
            "  ← você"
        } else {
            ""
        };
        println!(
            "  {:<20} {:<22} canal {}{}",
            user.user_id, user.name, user.channel_id, marca
        );
    }
    Ok(())
}

/// Sai da própria call via gateway.
pub(crate) fn leave(cli: &Cli) -> Result<()> {
    let token = token_of(cli).context("sem token")?;
    let (guild, channel) = scope_from_call(cli)?;
    gateway::leave(&token, uses_user_token(cli), &guild, 8)?;
    println!(
        "saí da call (guild {guild}, canal {})",
        channel.unwrap_or_default()
    );
    Ok(())
}

pub(crate) fn print_detect() -> Result<()> {
    let peers = detect::voice_peers()?;
    let voice: Vec<_> = peers.iter().filter(|p| p.is_voice()).collect();
    if voice.is_empty() {
        println!("nenhuma call ativa");
        if !peers.is_empty() {
            println!("sockets do Discord fora da faixa de voz (gateway/QUIC):");
            for peer in &peers {
                println!(
                    "  {}  pid={} ({})  local={}  {} kbps",
                    peer.endpoint(),
                    peer.pid,
                    peer.process,
                    peer.local_port,
                    peer.rate_kbps.unwrap_or(0)
                );
            }
        }
    } else {
        println!("call ativa, {} socket(s) de mídia:", voice.len());
        for peer in voice {
            println!(
                "  {}  pid={} ({})  local={}  {} kbps  confiança={}",
                peer.endpoint(),
                peer.pid,
                peer.process,
                peer.local_port,
                peer.rate_kbps.unwrap_or(0),
                peer.confidence()
            );
        }
    }
    Ok(())
}

pub(crate) fn uses_user_token(cli: &Cli) -> bool {
    cli.user_token || config::load().user_token
}

pub(crate) fn token_of(cli: &Cli) -> Option<String> {
    cli.token
        .clone()
        .or_else(|| std::env::var("DISCORD_TOKEN").ok())
        .or_else(|| config::load().token)
}

fn build_api(cli: &Cli) -> Result<Api> {
    let token = token_of(cli)
        .context("sem token — rode `silentium` e cole o seu token na primeira pergunta")?;
    Ok(Api::new(
        token,
        cli.dry_run,
        cli.delay_ms,
        uses_user_token(cli),
    ))
}

/// Guild/canal da call em que a conta está, descobertos pelo gateway.
pub(crate) fn scope_from_call(cli: &Cli) -> Result<(String, Option<String>)> {
    let token = token_of(cli).context("sem token para ler a call")?;
    let snap = gateway::snapshot(&token, uses_user_token(cli), 6)?;
    let mine = snap
        .users
        .iter()
        .find(|u| u.user_id == snap.me)
        .with_context(|| "a conta não está em nenhuma call de servidor")?;
    Ok((mine.guild_id.clone(), Some(mine.channel_id.clone())))
}

fn resolve_scope(api: &Api, cli: &Cli, target: &TargetArgs) -> Result<(String, Option<String>)> {
    if let Some(invite) = &target.invite {
        let resolved = api.resolve_invite(invite)?;
        let guild = resolved
            .guild
            .context("convite não aponta para um servidor")?;
        let channel = target
            .channel
            .clone()
            .or_else(|| resolved.channel.map(|c| c.id));
        return Ok((guild.id, channel));
    }
    if let Some(guild) = cli.guild.clone() {
        return Ok((guild, target.channel.clone()));
    }
    scope_from_call(cli)
}

fn collect_targets(
    api: &Api,
    guild: &str,
    channel: Option<&str>,
    explicit: &[String],
) -> Result<Vec<Target>> {
    if !explicit.is_empty() {
        let mut out = Vec::new();
        for uid in explicit {
            match api.voice_state(guild, uid)? {
                Some(state) => out.push(Target {
                    user_id: uid.clone(),
                    name: uid.clone(),
                    channel_id: state.channel_id.unwrap_or_default(),
                    mute: state.mute,
                    deaf: state.deaf,
                }),
                None => eprintln!("[!] {uid} não está em voz"),
            }
            pause(api);
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    for member in api.members(guild)? {
        if member.user.bot {
            continue;
        }
        if let Some(state) = api.voice_state(guild, &member.user.id)? {
            let channel_id = state.channel_id.clone().unwrap_or_default();
            if let Some(want) = channel {
                if channel_id != want {
                    continue;
                }
            }
            out.push(Target {
                user_id: member.user.id,
                name: member.user.username,
                channel_id,
                mute: state.mute,
                deaf: state.deaf,
            });
        }
        pause(api);
    }
    Ok(out)
}

fn pause(api: &Api) {
    if api.delay_ms > 0 {
        sleep(Duration::from_millis(api.delay_ms));
    }
}

pub(crate) fn voice_dispatch(cli: &Cli, cmd: &VoiceCmd) -> Result<()> {
    let api = build_api(cli)?;

    if let VoiceCmd::Guard {
        target,
        action,
        interval,
    } = cmd
    {
        return run_guard(&api, cli, target, action, *interval);
    }

    match cmd {
        VoiceCmd::Who => return who(cli),
        VoiceCmd::Leave => return leave(cli),
        _ => {}
    }

    let (target, action) = match cmd {
        VoiceCmd::List(t) => (t, "list"),
        VoiceCmd::Drop(t) => (t, "drop"),
        VoiceCmd::Mute(t) => (t, "mute"),
        VoiceCmd::Unmute(t) => (t, "unmute"),
        VoiceCmd::Deafen(t) => (t, "deafen"),
        VoiceCmd::Undeafen(t) => (t, "undeafen"),
        VoiceCmd::Restore(t) => (t, "restore"),
        VoiceCmd::Guard { .. } | VoiceCmd::Who | VoiceCmd::Leave => unreachable!(),
    };

    let (guild, channel) = match resolve_scope(&api, cli, target) {
        Ok(scope) => scope,
        Err(err) => {
            if action == "list" {
                return who(cli);
            }
            return Err(err);
        }
    };

    let targets = match collect_targets(&api, &guild, channel.as_deref(), &target.user) {
        Ok(list) => list,
        Err(_) if action == "list" => return who(cli),
        Err(err) => {
            if api.is_user_token() {
                bail!(
                    "token de usuário não altera outros membros (a API devolve 403).\n\
                     use `silentium voice leave` para sair da call ou `silentium net auto` para cortar.\n\
                     causa: {err:#}"
                );
            }
            return Err(err);
        }
    };

    if targets.is_empty() {
        println!("ninguém em voz no escopo");
        return Ok(());
    }

    if action == "list" {
        println!(
            "{:<20} {:<22} {:<20} mute  deaf",
            "user_id", "nome", "canal"
        );
        for t in &targets {
            println!(
                "{:<20} {:<22} {:<20} {:<5} {}",
                t.user_id, t.name, t.channel_id, t.mute, t.deaf
            );
        }
        println!("total: {}", targets.len());
        return Ok(());
    }

    let body = Api::body_for(action)?;
    println!("ação={action} alvos={}", targets.len());
    let mut ok = 0usize;
    for t in &targets {
        match api.patch_member(&guild, &t.user_id, &body) {
            Ok(()) => {
                ok += 1;
                println!("  ok    {} ({})", t.user_id, t.name);
            }
            Err(err) => println!("  falha {} ({}): {err}", t.user_id, t.name),
        }
        pause(&api);
    }
    println!("{ok}/{} aplicados", targets.len());
    Ok(())
}

fn run_guard(api: &Api, cli: &Cli, target: &TargetArgs, action: &str, interval: u64) -> Result<()> {
    let (guild, channel) = resolve_scope(api, cli, target)?;
    let body = Api::body_for(action)?;
    let mut seen: HashSet<String> = HashSet::new();

    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || flag.store(false, Ordering::SeqCst))
        .context("não consegui instalar handler de Ctrl-C")?;

    println!("[+] vigiando guild {guild} (ação: {action}, a cada {interval}s)");

    while running.load(Ordering::SeqCst) {
        match collect_targets(api, &guild, channel.as_deref(), &target.user) {
            Ok(current) => {
                for t in current {
                    if seen.insert(t.user_id.clone()) {
                        match api.patch_member(&guild, &t.user_id, &body) {
                            Ok(()) => println!("  {action} -> {} ({})", t.user_id, t.name),
                            Err(err) => println!("  falha em {}: {err}", t.user_id),
                        }
                    }
                }
            }
            Err(err) => eprintln!("[!] tick falhou: {err}"),
        }
        for _ in 0..interval {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            sleep(Duration::from_secs(1));
        }
    }

    println!("[+] guard encerrado");
    Ok(())
}

fn wait_window(seconds: Option<u64>, hint: &str) -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || flag.store(false, Ordering::SeqCst))
        .context("não consegui instalar handler de Ctrl-C")?;

    match seconds {
        Some(secs) => {
            println!("[+] ativo por {secs}s ({hint})");
            let mut left = secs;
            while left > 0 && running.load(Ordering::SeqCst) {
                sleep(Duration::from_secs(1));
                left -= 1;
            }
        }
        None => {
            println!("[+] ativo — {hint}");
            while running.load(Ordering::SeqCst) {
                sleep(Duration::from_millis(250));
            }
        }
    }
    Ok(())
}

fn net_dispatch(cmd: &NetCmd) -> Result<()> {
    match cmd {
        NetCmd::Clear { iface } => net::clear(iface.clone()),
        NetCmd::Unblock => net::unblock(),
        NetCmd::Auto { seconds, force } => {
            let peers: Vec<_> = detect::voice_peers()?
                .into_iter()
                .filter(|p| p.is_voice())
                .collect();
            if peers.is_empty() {
                if !force {
                    println!(
                        "[-] nenhuma call de voz detectada (use --force para cortar assim mesmo)"
                    );
                    return Ok(());
                }
                println!("[!] sem call de voz, cortando por --force");
            } else {
                let list: Vec<String> = peers.iter().map(|p| p.endpoint()).collect();
                println!("[+] call ativa: {}", list.join(", "));
            }
            net::block()?;
            let waited = wait_window(*seconds, "Enter ou Ctrl-C para restaurar");
            net::unblock().and(waited)
        }
        NetCmd::Block { seconds } => {
            net::block()?;
            let result = wait_window(*seconds, "Enter ou Ctrl-C para restaurar");
            let restored = net::unblock();
            result.and(restored)
        }
        NetCmd::Throttle {
            kbps,
            loss,
            delay,
            seconds,
            iface,
        } => {
            if *kbps == 0 {
                bail!("--kbps precisa ser maior que zero");
            }
            if let Some(loss) = loss {
                if *loss > 100 {
                    bail!("--loss vai de 0 a 100");
                }
            }
            let spec = net::ShapeSpec {
                rate_kbps: *kbps,
                loss_percent: *loss,
                delay_ms: *delay,
                iface: iface.clone(),
            };
            net::apply(&spec)?;

            let waited = wait_window(*seconds, "Ctrl-C restaura agora");
            net::clear(iface.clone()).and(waited)
        }
    }
}
