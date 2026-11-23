// rust — src/update.rs — detecta versão nova no GitHub e troca o próprio binário
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const REPO: &str = "onx90s/silentium";
pub const NPM_PACKAGE: &str = "@onxdev/silentium";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Duas checagens por dia, no máximo, para não bater no rate limit da API.
const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

#[derive(Debug, Serialize, Deserialize, Default)]
struct Cache {
    checked_at: u64,
    #[serde(default)]
    tag: Option<String>,
}

/// Entra na tela inicial: devolve a tag nova, se houver.
pub fn check(force: bool) -> Option<String> {
    let cache = read_cache();
    let fresh = now().saturating_sub(cache.checked_at) < CHECK_INTERVAL_SECS;
    if !force && fresh {
        return cache.tag.filter(|tag| is_newer(tag, CURRENT));
    }

    match fetch_latest_tag() {
        Ok(tag) => {
            write_cache(&Cache {
                checked_at: now(),
                tag: Some(tag.clone()),
            });
            if is_newer(&tag, CURRENT) {
                Some(tag)
            } else {
                None
            }
        }
        // Sem rede, offline ou rate limit: silêncio, o CLI segue funcionando.
        Err(_) => None,
    }
}

/// `silentium update` / `silentium update --check`.
pub fn run(check_only: bool) -> Result<()> {
    let latest = fetch_latest_tag().context("consultando a última release no GitHub")?;
    if !is_newer(&latest, CURRENT) {
        println!("já na última versão ({CURRENT})");
        return Ok(());
    }
    if check_only {
        println!("atualização disponível: {latest} (você tem {CURRENT})");
        return Ok(());
    }
    println!("atualizando {CURRENT} → {latest}");
    apply(&latest)
}

fn apply(tag: &str) -> Result<()> {
    if via_npm() {
        return update_via_npm(tag);
    }
    update_binary(tag)
}

/// Instalado como pacote npm: deixa o npm trocar o binário.
fn update_via_npm(tag: &str) -> Result<()> {
    let version = tag.trim_start_matches('v');
    let spec = format!("{NPM_PACKAGE}@{version}");
    println!("instalando {spec} via npm");
    let status = Command::new("npm")
        .args(["install", "-g", &spec])
        .status()
        .context("npm não encontrado no PATH")?;
    if !status.success() {
        bail!("npm install falhou ({status}); tente manualmente: npm i -g {spec}");
    }
    println!("pronto — a próxima execução já usa {tag}");
    Ok(())
}

fn update_binary(tag: &str) -> Result<()> {
    let asset = asset_name().context("plataforma sem binário publicado")?;
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let agent = agent();

    let dir = std::env::temp_dir().join(format!("silentium-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).context("criando diretório temporário")?;
    let archive = dir.join(&asset);

    let mut reader = agent
        .get(&format!("{base}/{asset}"))
        .call()
        .with_context(|| format!("baixando {asset}"))?
        .into_reader();
    let mut file = fs::File::create(&archive).context("criando arquivo temporário")?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        std::io::Write::write_all(&mut file, &buf[..read])?;
    }
    drop(file);

    if let Some(expected) = expected_sha(&agent, &base, &asset) {
        let actual = format!("{:x}", hasher.finalize());
        if expected != actual {
            bail!("checksum não bate: esperado {expected}, obtido {actual}");
        }
    } else {
        eprintln!("[!] SHASUMS256.txt indisponível, seguindo sem verificação");
    }

    extract(&archive, &dir)?;
    let new_bin = find_binary(&dir).context("binário não encontrado dentro do pacote")?;
    let replaced = replace_self(&new_bin);
    let _ = fs::remove_dir_all(&dir);
    replaced?;
    // o caminho antigo aparece como "… (deleted)" logo após o rename
    let shown = self_path()?
        .to_string_lossy()
        .replace(" (deleted)", "")
        .to_string();
    println!("pronto — {tag} instalado em {shown}");
    Ok(())
}

fn expected_sha(agent: &ureq::Agent, base: &str, asset: &str) -> Option<String> {
    let text = agent
        .get(&format!("{base}/SHASUMS256.txt"))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    text.lines()
        .find(|line| line.trim_end().ends_with(asset))
        .and_then(|line| line.split_whitespace().next())
        .map(|hash| hash.to_ascii_lowercase())
}

fn extract(archive: &Path, into: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    into.display()
                ),
            ])
            .status()
            .context("executando powershell")?;
        if !status.success() {
            bail!("Expand-Archive falhou");
        }
    }
    #[cfg(not(windows))]
    {
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(into)
            .status()
            .context("tar não encontrado")?;
        if !status.success() {
            bail!("tar falhou ao extrair {archive:?}");
        }
    }
    Ok(())
}

fn find_binary(dir: &Path) -> Option<PathBuf> {
    let name = binary_name();
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.file_name().map(|n| n == name).unwrap_or(false))
}

fn replace_self(new_bin: &Path) -> Result<()> {
    let current = self_path()?;
    let staged = current.with_file_name(format!(".{}.new", binary_name()));
    fs::copy(new_bin, &staged).context("copiando binário novo")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    }

    #[cfg(windows)]
    {
        let old = current.with_file_name(format!(".{}.old", binary_name()));
        let _ = fs::remove_file(&old);
        fs::rename(&current, &old).context("movendo binário em uso")?;
        if let Err(err) = fs::rename(&staged, &current) {
            let _ = fs::rename(&old, &current);
            return Err(err.into());
        }
        let _ = fs::remove_file(&old);
        return Ok(());
    }

    #[cfg(unix)]
    {
        fs::rename(&staged, &current)
            .with_context(|| format!("substituindo {}", current.display()))?;
        Ok(())
    }
}

fn self_path() -> Result<PathBuf> {
    std::env::current_exe().context("descobrindo o próprio executável")
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "silentium.exe"
    } else {
        "silentium"
    }
}

fn asset_name() -> Option<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    Some(format!("silentium-{os}-{arch}.{ext}"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
}

fn fetch_latest_tag() -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let json: serde_json::Value = agent()
        .get(&url)
        .set("user-agent", &format!("silentium/{CURRENT}"))
        .call()
        .context("GitHub API")?
        .into_json()
        .context("resposta da API não é JSON")?;
    json.get("tag_name")
        .and_then(|value| value.as_str())
        .map(|tag| tag.to_string())
        .context("release sem tag_name")
}

fn via_npm() -> bool {
    self_path()
        .map(|path| path.to_string_lossy().contains("node_modules"))
        .unwrap_or(false)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Option<PathBuf> {
    let base = cache_base()?;
    Some(base.join("silentium").join("update.json"))
}

fn cache_base() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))
    }
}

fn read_cache() -> Cache {
    cache_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_cache(cache: &Cache) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(cache) {
        let _ = fs::write(path, text);
    }
}

/// Compara tags tipo `v0.2.0`. Pré-release (`v0.2.0-rc.1`) conta como menor.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (latest_version, latest_pre) = parse_version(latest);
    let (current_version, current_pre) = parse_version(current);
    match latest_version.cmp(&current_version) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => current_pre && !latest_pre,
    }
}

fn parse_version(raw: &str) -> (Vec<u64>, bool) {
    let trimmed = raw.trim().trim_start_matches('v');
    let (core, pre) = match trimmed.split_once('-') {
        Some((core, _)) => (core, true),
        None => (trimmed, false),
    };
    let parts = core
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect();
    (parts, pre)
}

#[cfg(test)]
mod tests {
    use super::{is_newer, parse_version};

    #[test]
    fn reconhece_versao_maior() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("v0.1.10", "0.1.9"));
    }

    #[test]
    fn ignora_mesma_versao_e_menor() {
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "v0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
    }

    #[test]
    fn pre_release_e_tratada() {
        assert!(is_newer("v0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer("v0.2.0-rc.1", "0.2.0"));
    }

    #[test]
    fn versao_estavel_a_partir_de_pre() {
        let (atual, pre) = parse_version("v1.2.3-beta.2");
        assert_eq!(atual, vec![1, 2, 3]);
        assert!(pre);
    }
}
