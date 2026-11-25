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
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub discriminator: Option<String>,
}

impl Me {
    /// Nome exibido: global name quando existe; username#discriminator no legado.
    pub fn display_name(&self) -> String {
        if let Some(global) = self.global_name.as_deref().filter(|name| !name.is_empty()) {
            return global.to_string();
        }
        match self.discriminator.as_deref() {
            Some(tag) if tag != "0" && !tag.is_empty() => format!("{}#{tag}", self.username),
            _ => self.username.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub discriminator: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl User {
    pub fn display_name(&self) -> String {
        if let Some(global) = self.global_name.as_deref().filter(|name| !name.is_empty()) {
            return global.to_string();
        }
        match self.discriminator.as_deref() {
            Some(tag) if tag != "0" && !tag.is_empty() => format!("{}#{tag}", self.username),
            _ => self.username.clone(),
        }
    }
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
            .set("User-Agent", &user_agent(self.user_token.get()))
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

    pub fn guilds(&self) -> Result<Vec<(String, String)>> {
        let raw = self.raw("GET", "/users/@me/guilds", None)?;
        let list = raw.as_array().cloned().unwrap_or_default();
        Ok(list
            .iter()
            .map(|g| {
                (
                    g["id"].as_str().unwrap_or_default().to_string(),
                    g["name"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect())
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
            .set("User-Agent", &user_agent(self.user_token.get()));
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

/// Descobre se o token é de bot ou de usuário, e devolve o nome da conta.
pub fn probe(token: &str) -> Result<(bool, Me)> {
    let token = normalize_token(token)?;
    match probe_with(&token, true) {
        Ok(me) => return Ok((true, me)),
        Err(ProbeError::Unauthorized) => {}
        Err(err) => return Err(err.into()),
    }
    match probe_with(&token, false) {
        Ok(me) => Ok((false, me)),
        Err(ProbeError::Unauthorized) => Err(anyhow!(TokenRejected)),
        Err(err) => Err(err.into()),
    }
}

#[derive(Debug)]
pub struct TokenRejected;

impl std::fmt::Display for TokenRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a API retornou 401 — token inválido, revogado ou expirado")
    }
}

impl std::error::Error for TokenRejected {}

#[derive(Debug)]
enum ProbeError {
    Unauthorized,
    Http(u16, String),
    Network(String),
    Payload(String),
}

impl From<ProbeError> for anyhow::Error {
    fn from(err: ProbeError) -> Self {
        match err {
            ProbeError::Unauthorized => anyhow!(TokenRejected),
            ProbeError::Http(code, message) => {
                anyhow!("a API retornou HTTP {code}: {message}")
            }
            ProbeError::Network(message) => anyhow!("falha de rede ao validar: {message}"),
            ProbeError::Payload(message) => anyhow!("resposta inesperada de /users/@me: {message}"),
        }
    }
}

fn probe_with(token: &str, user_token: bool) -> std::result::Result<Me, ProbeError> {
    let auth = if user_token {
        token.to_string()
    } else {
        format!("Bot {token}")
    };
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build();
    let resp = agent
        .get(&format!("{API}/users/@me"))
        .set("Authorization", &auth)
        .set("User-Agent", &user_agent(user_token))
        .set("Accept", "*/*")
        .call()
        .map_err(|err| match err {
            ureq::Error::Status(401, _) => ProbeError::Unauthorized,
            ureq::Error::Status(code, resp) => {
                let payload = resp.into_json::<Value>().unwrap_or(Value::Null);
                let message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("sem mensagem")
                    .to_string();
                ProbeError::Http(code, message)
            }
            other => ProbeError::Network(other.to_string()),
        })?;
    resp.into_json::<Me>()
        .map_err(|err| ProbeError::Payload(err.to_string()))
}

/// UA válido para cada tipo de autenticação. Bots precisam conter "DiscordBot".
pub fn user_agent(user_token: bool) -> String {
    if user_token {
        let os = match std::env::consts::OS {
            "windows" => "Windows NT 10.0; Win64; x64",
            "macos" => "Macintosh; Intel Mac OS X 10_15_7",
            _ => "X11; Linux x86_64",
        };
        format!(
            "Mozilla/5.0 ({os}) AppleWebKit/537.36 (KHTML, like Gecko) discord/0.0.0 Chrome/120.0.0.0 Electron/28.0.0 Safari/537.36"
        )
    } else {
        format!(
            "DiscordBot (https://github.com/onx90s/silentium, {})",
            env!("CARGO_PKG_VERSION")
        )
    }
}

/// Aceita o token cru ou com prefixo/aspas e quebras de linha colados por engano.
pub fn normalize_token(raw: &str) -> Result<String> {
    let mut token: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    token = token.trim_matches(|c| c == '"' || c == '\'').to_string();
    for prefix in ["Bot", "Bearer"] {
        if token
            .get(..prefix.len())
            .map(|start| start.eq_ignore_ascii_case(prefix))
            .unwrap_or(false)
        {
            token = token[prefix.len()..].to_string();
            break;
        }
    }
    let valido = token.len() >= 40
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'='));
    if !valido {
        bail!("token incompleto — copie o valor inteiro, sem aspas");
    }
    Ok(token)
}

/// Detecta um token quebrado em duas linhas para continuar lendo o paste.
pub(crate) fn needs_continuation(raw: &str) -> bool {
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let compact = compact.trim_matches(|c| c == '"' || c == '\'');
    let compact = ["Bot", "Bearer"]
        .iter()
        .find_map(|prefix| {
            compact
                .get(..prefix.len())
                .filter(|start| start.eq_ignore_ascii_case(prefix))
                .map(|_| &compact[prefix.len()..])
        })
        .unwrap_or(compact);
    let parts: Vec<_> = compact.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    let first = parts[0];
    if !(20..=32).contains(&first.len())
        || !first
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return false;
    }
    parts.len() < 3 || compact.len() < 55 || parts[2].len() < 20
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

#[cfg(test)]
mod identity_tests {
    use super::{needs_continuation, normalize_token, Me, User};

    #[test]
    fn nome_de_exibicao_prioriza_global_name() {
        let me: Me = serde_json::from_value(serde_json::json!({
            "id": "1", "username": "antigo", "global_name": "Nome Correto", "discriminator": "0"
        }))
        .unwrap();
        assert_eq!(me.display_name(), "Nome Correto");
    }

    #[test]
    fn discriminador_legado_e_preservado() {
        let me: Me = serde_json::from_value(serde_json::json!({
            "id": "1", "username": "antigo", "global_name": null, "discriminator": "1234"
        }))
        .unwrap();
        assert_eq!(me.display_name(), "antigo#1234");
    }

    #[test]
    fn usuario_tambem_usa_global_name() {
        let user: User = serde_json::from_value(serde_json::json!({
            "id": "1", "username": "antigo", "global_name": "Visível", "discriminator": "0"
        }))
        .unwrap();
        assert_eq!(user.display_name(), "Visível");
    }

    #[test]
    fn normaliza_prefixo_e_aspas() {
        let token = "a".repeat(48);
        let raw = format!("  \"Bot {token}\"  ");
        assert_eq!(normalize_token(&raw).unwrap(), token);
        assert_eq!(
            normalize_token(&format!("'Bearer {token}'")).unwrap(),
            token
        );
    }

    #[test]
    fn normaliza_quebras_de_linha_no_token() {
        let token = format!("{}abc.def", "a".repeat(48));
        let raw = format!("\"{}\n{}\t{}\"", &token[..30], &token[30..55], &token[55..]);
        assert_eq!(normalize_token(&raw).unwrap(), token);
    }

    #[test]
    fn detecta_token_quebrado_em_duas_linhas() {
        let token = format!("{}.abcdef.{}", "a".repeat(26), "b".repeat(38));
        assert!(needs_continuation(&token[..38]));
        assert!(!needs_continuation(&token));
        assert!(!needs_continuation("abc"));
    }

    #[test]
    fn rejeita_token_curto() {
        assert!(normalize_token("abc").is_err());
    }

    #[test]
    fn user_agent_de_bot_contem_discordbot() {
        assert!(super::user_agent(false).contains("DiscordBot"));
    }
}
