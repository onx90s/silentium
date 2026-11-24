// rust — src/gateway.rs — snapshot de voz via Gateway (funciona com token de usuário)
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tungstenite::{connect, Message};

const URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json";

#[derive(Debug, Clone)]
pub struct VoiceUser {
    pub user_id: String,
    pub name: String,
    pub channel_id: String,
    pub guild_id: String,
    #[allow(dead_code)]
    pub self_stream: bool,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub me: String,
    pub users: Vec<VoiceUser>,
    pub guilds: Vec<(String, String)>,
}

pub fn snapshot(token: &str, user_token: bool, seconds: u64) -> Result<Snapshot> {
    match snapshot_try(token, user_token, seconds) {
        Ok(snap) => Ok(snap),
        Err(first) => match snapshot_try(token, !user_token, seconds) {
            Ok(snap) => Ok(snap),
            Err(_) => Err(first),
        },
    }
}

fn snapshot_try(token: &str, user_token: bool, seconds: u64) -> Result<Snapshot> {
    let (mut socket, _) = connect(URL).context("não conectei ao gateway")?;
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(tcp) => {
            tcp.set_read_timeout(Some(Duration::from_millis(400)))?;
        }
        tungstenite::stream::MaybeTlsStream::Rustls(tls) => {
            tls.get_ref()
                .set_read_timeout(Some(Duration::from_millis(400)))?;
        }
        _ => {}
    }

    let deadline = Instant::now() + Duration::from_secs(seconds.max(3));
    let mut interval = 41.25_f64;
    let mut next_beat = Instant::now() + Duration::from_millis(41250);
    let mut identified = false;
    let mut me = String::new();
    let mut users: Vec<VoiceUser> = Vec::new();
    let mut guilds: Vec<(String, String)> = Vec::new();

    while Instant::now() < deadline {
        if !identified {
            let hello = next_json(&mut socket)?;
            interval = hello
                .pointer("/d/heartbeat_interval")
                .and_then(Value::as_f64)
                .unwrap_or(41250.0);
            next_beat = Instant::now() + Duration::from_millis(interval as u64);

            let auth = if user_token {
                token.to_string()
            } else {
                format!("Bot {token}")
            };
            let payload = json!({
                "op": 2,
                "d": {
                    "token": auth,
                    "capabilities": 30717,
                    "properties": {
                        "os": std::env::consts::OS,
                        "browser": "Discord Client",
                        "device": ""
                    },
                    "compress": false
                }
            });
            socket.send(Message::Text(payload.to_string().into()))?;
            identified = true;
        }

        if Instant::now() >= next_beat {
            socket.send(Message::Text(
                json!({"op": 1, "d": Value::Null}).to_string().into(),
            ))?;
            next_beat = Instant::now() + Duration::from_millis(interval as u64);
        }

        let msg = match socket.read() {
            Ok(Message::Text(text)) => text.to_string(),
            Ok(Message::Binary(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
            Ok(Message::Close(frame)) => bail!("gateway fechou: {frame:?}"),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(err))
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(err) => bail!("gateway: {err}"),
        };

        let value: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match value.get("op").and_then(Value::as_i64) {
            Some(9) => bail!("identify rejeitado (token inválido ou já expirado)"),
            Some(7) => bail!("gateway pediu reconexão"),
            _ => {}
        }

        match value.get("t").and_then(Value::as_str) {
            Some("READY") => {
                let d = &value["d"];
                me = d["user"]["id"].as_str().unwrap_or_default().to_string();
                for guild in d["guilds"].as_array().cloned().unwrap_or_default() {
                    let gid = guild["id"].as_str().unwrap_or_default().to_string();
                    let gname = guild["properties"]["name"]
                        .as_str()
                        .unwrap_or(&gid)
                        .to_string();
                    guilds.push((gid.clone(), gname));
                    collect(guild.get("voice_states"), &gid, &mut users);
                }
            }
            Some("VOICE_STATE_UPDATE") => {
                let d = &value["d"];
                let gid = d["guild_id"].as_str().unwrap_or_default().to_string();
                if gid.is_empty() {
                    continue;
                }
                let uid = d["user_id"].as_str().unwrap_or_default().to_string();
                let channel = d["channel_id"].as_str().unwrap_or_default().to_string();
                if channel.is_empty() {
                    users.retain(|u| u.user_id != uid);
                    continue;
                }
                let name = display_name(&d["member"], &uid);
                upsert(
                    &mut users,
                    VoiceUser {
                        user_id: uid,
                        name,
                        channel_id: channel,
                        guild_id: gid,
                        self_stream: d["self_stream"].as_bool().unwrap_or(false),
                    },
                );
            }
            _ => {}
        }

        if me.is_empty() {
            continue;
        }
        if Instant::now() + Duration::from_millis(900) > deadline {
            break;
        }
    }

    let _ = socket.close(None);
    users.sort_by(|a, b| a.guild_id.cmp(&b.guild_id).then(a.name.cmp(&b.name)));
    Ok(Snapshot { me, users, guilds })
}

fn display_name(member: &Value, fallback: &str) -> String {
    if let Some(nick) = member["nick"].as_str().filter(|name| !name.is_empty()) {
        return nick.to_string();
    }
    if let Some(global) = member["user"]["global_name"]
        .as_str()
        .filter(|name| !name.is_empty())
    {
        return global.to_string();
    }
    let username = member["user"]["username"].as_str().unwrap_or(fallback);
    let tag = member["user"]["discriminator"].as_str().unwrap_or("0");
    if tag != "0" && !tag.is_empty() {
        format!("{username}#{tag}")
    } else {
        username.to_string()
    }
}

fn collect(states: Option<&Value>, gid: &str, out: &mut Vec<VoiceUser>) {
    let Some(list) = states.and_then(Value::as_array) else {
        return;
    };
    for state in list {
        let channel = state["channel_id"].as_str().unwrap_or_default();
        if channel.is_empty() {
            continue;
        }
        let uid = state["user_id"].as_str().unwrap_or_default().to_string();
        let name = display_name(&state["member"], &uid);
        upsert(
            out,
            VoiceUser {
                user_id: uid,
                name,
                channel_id: channel.to_string(),
                guild_id: gid.to_string(),
                self_stream: state["self_stream"].as_bool().unwrap_or(false),
            },
        );
    }
}

fn upsert(out: &mut Vec<VoiceUser>, user: VoiceUser) {
    if let Some(slot) = out.iter_mut().find(|u| u.user_id == user.user_id) {
        *slot = user;
    } else {
        out.push(user);
    }
}

fn next_json(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> Result<Value> {
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    return Ok(v);
                }
            }
            Ok(Message::Binary(bytes)) => {
                if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                    return Ok(v);
                }
            }
            Ok(_) => continue,
            Err(tungstenite::Error::Io(err))
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(err) => bail!("gateway: {err}"),
        }
    }
}

/// Sai da call do servidor informado (op 4 com channel_id nulo).
pub fn leave(token: &str, user_token: bool, guild_id: &str, seconds: u64) -> Result<()> {
    let (mut socket, _) = connect(URL).context("não conectei ao gateway")?;
    let deadline = Instant::now() + Duration::from_secs(seconds.max(5));
    let auth = if user_token {
        token.to_string()
    } else {
        format!("Bot {token}")
    };

    let hello = next_json(&mut socket)?;
    let interval = hello
        .pointer("/d/heartbeat_interval")
        .and_then(Value::as_f64)
        .unwrap_or(41250.0) as u64;
    let mut next_beat = Instant::now() + Duration::from_millis(interval);

    socket.send(Message::Text(
        json!({
            "op": 2,
            "d": {
                "token": auth,
                "capabilities": 30717,
                "properties": {
                        "os": std::env::consts::OS,
                        "browser": "Discord Client",
                        "device": ""
                    },
                "compress": false
            }
        })
        .to_string()
        .into(),
    ))?;

    let mut sent = false;
    while Instant::now() < deadline {
        if Instant::now() >= next_beat {
            socket.send(Message::Text(
                json!({"op": 1, "d": Value::Null}).to_string().into(),
            ))?;
            next_beat = Instant::now() + Duration::from_millis(interval);
        }

        let frame = match socket.read() {
            Ok(Message::Text(text)) => text.to_string(),
            Ok(Message::Binary(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(err))
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(err) => bail!("gateway: {err}"),
        };

        let value: Value = match serde_json::from_str(&frame) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if value.get("op").and_then(Value::as_i64) == Some(9) {
            bail!("gateway recusou o token");
        }
        if value.get("t").and_then(Value::as_str) == Some("READY") && !sent {
            socket.send(Message::Text(
                json!({
                    "op": 4,
                    "d": {
                        "guild_id": guild_id,
                        "channel_id": Value::Null,
                        "self_mute": false,
                        "self_deaf": false
                    }
                })
                .to_string()
                .into(),
            ))?;
            sent = true;
            std::thread::sleep(Duration::from_millis(700));
            break;
        }
    }

    let _ = socket.close(None);
    if sent {
        Ok(())
    } else {
        bail!("não consegui enviar o comando de saída")
    }
}

#[cfg(test)]
mod tests {
    use super::display_name;
    use serde_json::json;

    #[test]
    fn apelido_do_servidor_tem_prioridade() {
        let member =
            json!({"nick": "Apelido", "user": {"username": "user", "global_name": "Global"}});
        assert_eq!(display_name(&member, "1"), "Apelido");
    }

    #[test]
    fn global_name_vem_antes_do_username() {
        let member = json!({"nick": null, "user": {"username": "user", "global_name": "Global", "discriminator": "0"}});
        assert_eq!(display_name(&member, "1"), "Global");
    }

    #[test]
    fn tag_legada_e_preservada() {
        let member =
            json!({"user": {"username": "user", "global_name": null, "discriminator": "1234"}});
        assert_eq!(display_name(&member, "1"), "user#1234");
    }
}
