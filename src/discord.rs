use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::thread::sleep;
use std::time::Duration;

const API: &str = "https://discord.com/api/v10";

#[derive(Debug, Deserialize)]
pub struct Me {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub bot: bool,
}

#[derive(Debug, Deserialize)]
pub struct Member {
    pub user: User,
}

#[derive(Debug, Deserialize)]
pub struct VoiceState {
    #[allow(dead_code)]
    pub user_id: String,
    pub channel_id: Option<String>,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub deaf: bool,
}

#[derive(Debug, Deserialize)]
pub struct InviteTarget {
    pub guild: Option<InviteGuild>,
    pub channel: Option<InviteChannel>,
}

#[derive(Debug, Deserialize)]
pub struct InviteGuild {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct InviteChannel {
    pub id: String,
    pub name: String,
}

pub struct Api {
    agent: ureq::Agent,
    token: String,
    /// `true` = token de usuário (header cru); `false` = bot (`Bot <token>`).
    user_token: std::cell::Cell<bool>,
    trocou: std::cell::Cell<bool>,
    pub dry_run: bool,
    pub delay_ms: u64,
}

impl Api {
    pub fn new(token: String, dry_run: bool, delay_ms: u64, user_token: bool) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build();
        Self {
            agent,
            token,
            user_token: std::cell::Cell::new(user_token),
            trocou: std::cell::Cell::new(false),
            dry_run,
            delay_ms,
        }
    }

    fn header(&self) -> String {
        if self.user_token.get() {
            self.token.clone()
        } else {
            format!("Bot {}", self.token)
        }
    }

    /// 401 na primeira tentativa = tipo de token errado. Troca e segue.
    fn alternar(&self) -> bool {
        if self.trocou.get() {
            return false;
        }
        self.trocou.set(true);
        self.user_token.set(!self.user_token.get());
        true
    }

    pub fn is_user_token(&self) -> bool {
        self.user_token.get()
    }

    fn auth(&self, req: ureq::Request) -> ureq::Request {
        req.set("Authorization", &self.header())
            .set("User-Agent", "Silentium/0.1 (local)")
            .set("Content-Type", "application/json")
    }

    fn raw(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let url = format!("{API}{path}");
        loop {
            let req = self.auth(self.agent.request(method, &url));
            let sent = match &body {
                Some(payload) => req.send_json(payload.clone()),
                None => req.call(),
            };
            match sent {
                Ok(resp) => {
                    if resp.status() == 204 {
                        return Ok(Value::Null);
                    }
                    return resp.into_json::<Value>().context("resposta não-JSON");
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let payload = resp.into_json::<Value>().unwrap_or(Value::Null);
                    if code == 401 && self.alternar() {
                        continue;
                    }
                    if code == 429 {
                        let wait = payload
                            .get("retry_after")
                            .and_then(Value::as_f64)
                            .unwrap_or(1.5);
                        sleep(Duration::from_millis((wait * 1000.0) as u64 + 120));
                        continue;
                    }
                    let msg = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("sem mensagem");
                    bail!("{method} {path} -> HTTP {code}: {msg}");
                }
                Err(err) => bail!("{method} {path} falhou: {err}"),
            }
        }
    }

    fn pause(&self) {
        if self.delay_ms > 0 {
            sleep(Duration::from_millis(self.delay_ms));
        }
    }

    pub fn me(&self) -> Result<Me> {
        let raw = self.raw("GET", "/users/@me", None)?;
        serde_json::from_value(raw).context("payload de /users/@me inesperado")
    }

    pub fn resolve_invite(&self, invite: &str) -> Result<InviteTarget> {
        let code = invite_code(invite)?;
        let raw = self.raw("GET", &format!("/invites/{code}?with_counts=true"), None)?;
        serde_json::from_value(raw).context("payload de /invites inesperado")
    }

    pub fn members(&self, guild: &str) -> Result<Vec<Member>> {
        let mut out: Vec<Member> = Vec::new();
        let mut after = String::from("0");
        loop {
            let page = self.raw(
                "GET",
                &format!("/guilds/{guild}/members?limit=1000&after={after}"),
                None,
            )?;
            let batch: Vec<Member> =
                serde_json::from_value(page).context("payload de /members inesperado")?;
            let len = batch.len();
            if let Some(last) = batch.last() {
                after.clone_from(&last.user.id);
            }
            out.extend(batch);
            if len < 1000 {
                break;
            }
            self.pause();
        }
        Ok(out)
    }

    pub fn voice_state(&self, guild: &str, user: &str) -> Result<Option<VoiceState>> {
        let url = format!("{API}/guilds/{guild}/voice-states/{user}");
        let req = self
            .agent
            .get(&url)
            .set("Authorization", &self.header())
            .set("User-Agent", "Silentium/0.1 (local)");
        match req.call() {
            Ok(resp) => {
                let payload = resp.into_json::<Value>().unwrap_or(Value::Null);
                Ok(serde_json::from_value::<VoiceState>(payload).ok())
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(code, resp)) => {
                let payload = resp.into_json::<Value>().unwrap_or(Value::Null);
                let msg = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("sem mensagem");
                bail!("GET voice-states -> HTTP {code}: {msg}");
            }
            Err(err) => Err(anyhow!("GET voice-states falhou: {err}")),
        }
    }

    pub fn patch_member(&self, guild: &str, user: &str, body: &Value) -> Result<()> {
        if self.dry_run {
            println!("  [dry-run] PATCH /guilds/{guild}/members/{user} {body}");
            return Ok(());
        }
        self.raw(
            "PATCH",
            &format!("/guilds/{guild}/members/{user}"),
            Some(body.clone()),
        )?;
        Ok(())
    }

    pub fn body_for(action: &str) -> Result<Value> {
        Ok(match action {
            "drop" => json!({ "channel_id": null }),
            "mute" => json!({ "mute": true }),
            "unmute" => json!({ "mute": false }),
            "deafen" => json!({ "deaf": true }),
            "undeafen" => json!({ "deaf": false }),
            "restore" => json!({ "mute": false, "deaf": false }),
            other => bail!("ação desconhecida: {other}"),
        })
    }
}

pub fn invite_code(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let tail = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let code = tail.split(['?', '#']).next().unwrap_or(tail);
    let valido = !code.is_empty() && code.len() <= 64 && !code.contains('.') && !code.contains(':');
    if !valido {
        bail!("convite inválido: {raw}");
    }
    Ok(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::{invite_code, Api};
    use serde_json::json;

    #[test]
    fn extrai_codigo_de_varias_formas() {
        for raw in [
            "discord.gg/abc123",
            "https://discord.gg/abc123",
            "https://discord.gg/abc123/",
            "https://discord.com/invite/abc123?event=1",
            "abc123",
        ] {
            assert_eq!(invite_code(raw).unwrap(), "abc123", "entrada: {raw}");
        }
    }

    #[test]
    fn rejeita_convite_vazio() {
        assert!(invite_code("https://discord.gg/").is_err());
        assert!(invite_code("").is_err());
    }

    #[test]
    fn corpo_do_drop_zera_canal() {
        assert_eq!(
            Api::body_for("drop").unwrap(),
            json!({ "channel_id": null })
        );
    }

    #[test]
    fn corpo_do_restore_limpa_estado() {
        assert_eq!(
            Api::body_for("restore").unwrap(),
            json!({ "mute": false, "deaf": false })
        );
    }

    #[test]
    fn acao_invalida_falha() {
        assert!(Api::body_for("explodir").is_err());
        assert!(Api::body_for("drop").is_ok());
    }
}

/// Descobre se o token é de bot ou de usuário, e devolve o nome da conta.
pub fn probe(token: &str) -> Result<(bool, String)> {
    if let Ok(name) = probe_with(token, true) {
        return Ok((false, name));
    }
    let name = probe_with(token, false).context("token recusado pela API")?;
    Ok((true, name))
}

fn probe_with(token: &str, bot: bool) -> Result<String> {
    let auth = if bot {
        format!("Bot {token}")
    } else {
        token.to_string()
    };
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build();
    let resp = agent
        .get(&format!("{API}/users/@me"))
        .set("Authorization", &auth)
        .set("User-Agent", "Silentium")
        .call()
        .map_err(|err| anyhow!("{err}"))?;
    let value: Value = resp.into_json().context("resposta inesperada")?;
    let name = value
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("conta")
        .to_string();
    Ok(name)
}
