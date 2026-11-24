// rust — src/logs.rs — descobre a call pelos logs do próprio cliente
use anyhow::Result;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Call {
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub media_server: Option<String>,
    pub since: Option<String>,
}

impl Call {
    pub fn describe(&self) -> String {
        let mut partes = Vec::new();
        if let Some(guild) = &self.guild_id {
            partes.push(format!("guild {guild}"));
        }
        if let Some(channel) = &self.channel_id {
            partes.push(format!("canal {channel}"));
        }
        if let Some(server) = &self.media_server {
            partes.push(format!("mídia {server}"));
        }
        if let Some(since) = &self.since {
            partes.push(format!("desde {since}"));
        }
        partes.join(" · ")
    }
}

/// Diretórios de log do cliente Discord, por plataforma.
fn log_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(not(windows))]
    let home = std::env::var("HOME").unwrap_or_default();

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from(&home).join(".var/app/com.discordapp.Discord/config/discord/logs"));
        dirs.push(PathBuf::from(&home).join(".config/discord/logs"));
        dirs.push(PathBuf::from(&home).join(".config/discordcanary/logs"));
        dirs.push(PathBuf::from(&home).join(".config/discordptb/logs"));
    }
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA").unwrap_or_default();
        for nome in [
            "discord",
            "discordptb",
            "discordcanary",
            "discorddevelopment",
        ] {
            dirs.push(PathBuf::from(&base).join(nome).join("logs"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from(&home).join("Library/Application Support/discord/logs"));
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

fn tail(path: &PathBuf, limit: u64) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(limit);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).to_string())
}

/// Última conexão de voz que continua aberta, lida dos logs do client.
pub fn current_call() -> Result<Option<Call>> {
    let mut melhor: Option<Call> = None;

    for dir in log_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let nome = entry.file_name().to_string_lossy().to_string();
            if !nome.ends_with(".log") {
                continue;
            }
            let Some(texto) = tail(&entry.path(), 4 * 1024 * 1024) else {
                continue;
            };
            if let Some(call) = parse(&texto) {
                melhor = Some(call);
            }
        }
    }
    Ok(melhor)
}

fn parse(texto: &str) -> Option<Call> {
    let mut conectou: Option<(String, Call)> = None;
    let mut desconectou = String::new();
    let mut canal: Option<String> = None;

    for linha in texto.lines() {
        let quando = linha
            .strip_prefix('[')
            .and_then(|rest| rest.split(']').next())
            .unwrap_or("")
            .to_string();

        if linha.contains("RTC connected to media server") {
            let guild = linha
                .split("RTCConnection(")
                .nth(1)
                .and_then(|rest| rest.split(',').next())
                .map(str::to_string);
            let server = linha
                .rsplit("media server: ")
                .next()
                .map(|s| s.trim().to_string());
            conectou = Some((
                quando.clone(),
                Call {
                    guild_id: guild,
                    channel_id: None,
                    media_server: server,
                    since: Some(quando.clone()),
                },
            ));
        }

        if linha.contains("VOICE_DISCONNECT") || linha.contains("RTC_DISCONNECTED =>") {
            desconectou = quando.clone();
        }

        if let Some(pos) = linha.find("\"channelId\":\"") {
            let resto = &linha[pos + 13..];
            if let Some(fim) = resto.find('"') {
                canal = Some(resto[..fim].to_string());
            }
        }
    }

    let (quando, mut call) = conectou?;
    if !desconectou.is_empty() && desconectou > quando {
        return None;
    }
    call.channel_id = canal;
    Some(call)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn detecta_conexao_viva() {
        let log = "\
[2026-11-24 14:51:32.734] [info]  [RTCConnection(1574184821347975221, default)] RTC connected to media server: 104.29.142.231:19329
[2026-11-24 14:51:37.786] [error] [AVError] AV error reported: no-audio-input-detected {\"channelId\":\"1574184822199291937\"}
";
        let call = parse(log).expect("call ativa");
        assert_eq!(call.guild_id.as_deref(), Some("1574184821347975221"));
        assert_eq!(call.channel_id.as_deref(), Some("1574184822199291937"));
        assert_eq!(call.media_server.as_deref(), Some("104.29.142.231:19329"));
    }

    #[test]
    fn desconexao_depois_encerra() {
        let log = "\
[2026-11-24 14:51:32.734] [info]  [RTCConnection(1, default)] RTC connected to media server: 1.2.3.4:5000
[2026-11-24 14:53:10.631] [info]  [RTCConnection(1, default)] [VOICE_DISCONNECT] reason
";
        assert!(parse(log).is_none());
    }

    #[test]
    fn sem_conexao() {
        assert!(parse("nada aqui").is_none());
    }
}
