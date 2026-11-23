# Silentium

```
 ___   ___   _     ___   _  _   _____   ___   _   _   __  __
/ __| |_ _| | |   | __| | \| | |_   _| |_ _| | | | | |  \/  |
\__ \  | |  | |__ | _|  | .` |   | |    | |  | |_| | | |\/| |
|___/ |___| |____ |___| |_|\_|   |_|   |___|  \___/  |_|  |_|
```

**Kill switch de voz para Discord.** Detecta a call ativa pelos sockets locais do cliente e corta, limita, muta ou derruba — sem plugin, sem injeção, sem tocar no aplicativo.

[![release](https://img.shields.io/github/v/release/onx90s/silentium?color=6c8cff)](https://github.com/onx90s/silentium/releases)
[![npm](https://img.shields.io/npm/v/@onx90s/silentium?label=npm&color=cb3837)](https://www.npmjs.com/package/@onx90s/silentium)
[![ci](https://img.shields.io/github/actions/workflow/status/onx90s/silentium/ci.yml?label=ci)](https://github.com/onx90s/silentium/actions)
[![license](https://img.shields.io/badge/license-MIT-8fbcbb)](#licença)
![platforms](https://img.shields.io/badge/plataformas-linux%20·%20windows%20·%20macos-9aa0a6)

---

## O que é

Silentium é uma CLI em Rust que trata a voz do Discord como infraestrutura: você escolhe o escopo — todos na call, um canal, um usuário — e aplica uma ação.

| camada | o que faz | precisa de |
|---|---|---|
| **detecção local** | descobre se existe call ativa e onde, lendo os sockets UDP do processo Discord | nada |
| **controle de voz** | derruba, muta, ensurdece, restaura e vigia entrada de participantes via REST | token com permissões de moderação |
| **shaping de rede** | corta ou limita só o tráfego do Discord, o resto da máquina segue intacto | root (Linux) / Administrador (Windows) |

O binário é pequeno, sem runtime, sem dependência de Node ou Python. `npm install -g silentium` existe só como transporte do binário.

## Como funciona

**1. Detecção sem API.** O Discord negocia mídia em UDP na faixa **50000–65535**. Silentium lê a tabela de sockets do sistema, filtra o que pertence ao processo Discord e separa tráfego de mídia (faixa de voz) de tráfego de gateway/QUIC (443). Nenhum token é usado nesse caminho — `silentium detect` funciona offline.

**2. Controle via REST v10.** Com token de bot (`Authorization: Bot <token>`) ou token de usuário (`--user-token`), o comando faz `PATCH /guilds/{guild}/members/{user}` com um destes corpos:

```json
{ "channel_id": null }   // derruba da call
{ "mute": true }         // muta no servidor
{ "deaf": true }         // ensurdece no servidor
{ "mute": false, "deaf": false }  // restaura
```

Permissões exigidas no servidor: `MOVE_MEMBERS`, `MUTE_MEMBERS`, `DEAFEN_MEMBERS`.

**3. Shaping no sistema.** No Linux, `iptables` com match por dono do socket e `tc`/`netem` para banda, perda e latência. No Windows, regras de firewall por caminho do `Discord.exe` e política `New-NetQosPolicy` na faixa de voz. Sempre restrito ao tráfego do Discord.

## Instalação

### npm (recomendado)

```bash
npm install -g @onx90s/silentium
silentium
```

O `postinstall` baixa o binário pré-compilado para a sua plataforma direto dos releases, confere o SHA-256 e coloca em `node_modules/@onx90s/silentium/vendor/`. Sem Rust instalado, sem compilação. Se o download falhar e houver Rust na máquina, ele compila na hora.

Linux, macOS e Windows usam exatamente o mesmo comando. Precisa de Node 18+ apenas para instalar; o binário em si não tem runtime.

Sem instalar nada globalmente:

```bash
npx @onx90s/silentium
```

> O nome `silentium` no npm pertence a outro projeto (uma solução de mensageria). Este CLI é publicado sob o escopo `@onx90s`, e o comando continua sendo `silentium`.

### Cargo

```bash
cargo install --git https://github.com/onx90s/silentium --locked
```

### Binário direto

```bash
# Linux x86_64
curl -fsSL https://github.com/onx90s/silentium/releases/latest/download/silentium-linux-x64.tar.gz \
  | tar -xz -C ~/.local/bin
chmod +x ~/.local/bin/silentium
```

Assets disponíveis: `silentium-linux-x64`, `silentium-linux-arm64`, `silentium-darwin-x64`, `silentium-darwin-arm64`, `silentium-windows-x64.zip`, sempre com `SHASUMS256.txt` ao lado.

## Primeiro uso

Rodar sem argumento abre a tela inicial: banner, diagnóstico do sistema e menu.

```bash
silentium
```

```
  sistema    linux-x86_64 · uid 1000
  discord    ● rodando — pid 4213
  call       ● ativa — 2 socket(s) de mídia (50002, 50004)
  token      ● definido (MTQ3…)
  guild      ● 123456789012345678
  firewall   ○ sem privilégio — net só com sudo
```

Depois, dois caminhos:

```bash
# só detecção, zero credencial
silentium detect

# controle de voz: aponte o convite da call, sem precisar de IDs
export DISCORD_TOKEN="seu_token_de_bot"
silentium voice list  --invite discord.gg/abc123
silentium voice drop  --invite discord.gg/abc123
```

`--invite` resolve o convite em guild + canal automaticamente. IDs explícitos também valem:

```bash
silentium voice mute --guild 123456789012345678 --channel 987654321098765432
```

## Comandos

### Globais

| flag | padrão | função |
|---|---|---|
| `--token` | `$DISCORD_TOKEN` | token de bot, ou de usuário com `--user-token` |
| `--user-token` | — | trata o token como de usuário (sem prefixo `Bot `) |
| `--guild` | `$GUILD_ID` | ID do servidor |
| `--delay-ms` | `150` | intervalo entre chamadas REST |
| `--dry-run` | — | mostra o que faria, sem executar |

### `silentium detect`

Lista sockets do Discord. Sem token. Marca o que cai na faixa de voz.

```bash
silentium detect
```

### `silentium whoami` / `silentium resolve`

```bash
silentium whoami                          # valida o token
silentium resolve discord.gg/abc123       # convite -> guild + canal
```

### `silentium voice <ação>`

Ações: `list`, `drop`, `mute`, `unmute`, `deafen`, `undeafen`, `restore`.

```bash
silentium voice list                              # quem está em voz
silentium voice drop                              # derruba todos
silentium voice mute --channel 987654321098765432 # muta um canal
silentium voice restore                           # desfaz mute e deafen
silentium voice drop --user 111111111111111111    # alvo único
```

Filtros comuns a todas as ações:

| flag | efeito |
|---|---|
| `--guild <id>` | servidor |
| `--channel <id>` | restringe a um canal de voz |
| `--user <id>` | alvo específico, repetível |
| `--invite <link>` | extrai guild e canal do convite |

### `silentium voice guard`

Vigia o canal e aplica a ação em quem entrar.

```bash
silentium voice guard --invite discord.gg/abc123 --action drop --interval 2
```

### `silentium net <subcomando>`

| comando | efeito |
|---|---|
| `throttle --kbps 8` | limita a banda do áudio (8 degrada, 2 mata) |
| `throttle --loss 30 --latency 400` | perda e latência no UDP de voz (Linux) |
| `clear` | remove todo o shaping |
| `block` | corta a internet **só do Discord**, por processo |
| `unblock` | desfaz o bloqueio |
| `auto` | detecta call ativa e corta; restaura no `Enter`/`Ctrl-C` |

```bash
sudo silentium net auto --seconds 30
sudo silentium net throttle --kbps 4 --loss 40
```

## Receitas

Cortar a call por 30 segundos, sem pensar em IDs:

```bash
sudo silentium net auto --seconds 30
```

Silenciar um participante específico, sem sair do servidor:

```bash
silentium voice mute --invite discord.gg/abc123 --user 987654321098765432
silentium voice unmute --invite discord.gg/abc123 --user 987654321098765432
```

Receber ninguém: vigia o canal e derruba quem entrar.

```bash
silentium voice guard --invite discord.gg/abc123 --action drop
```

Silêncio total numa máquina compartilhada: corta o Discord, resto navega normal.

```bash
sudo silentium net block          # até Enter/Ctrl-C
sudo silentium net block --seconds 120
```

## Permissões

| operação | Linux | Windows |
|---|---|---|
| `detect` | `iproute2` (`ss`) | PowerShell (`netstat`) |
| `voice *` | token + permissões no servidor | idem |
| `net block` | root, `iptables` | Administrador, firewall |
| `net throttle` | root, `iptables` + `tc`/`netem` | Administrador, `New-NetQosPolicy` (só banda) |
| `net throttle --loss/--latency` | Linux apenas | ignorado com aviso |

No Windows a pilha nativa de QoS só limita banda; perda e latência são exclusivas do Linux.

## Plataformas

| | detect | voice | net block | net throttle |
|---|---|---|---|---|
| Linux x86_64 / arm64 | ✅ | ✅ | ✅ | ✅ |
| Windows x86_64 | ✅ | ✅ | ✅ | banda |
| macOS x86_64 / arm64 | ⚠️ `detect` usa `ss`, indisponível | ✅ | — | — |

O controle de voz é independente de plataforma: é REST.

## Construindo do código

```bash
git clone https://github.com/onx90s/silentium
cd silentium
cargo build --release      # target/release/silentium
cargo test
```

Toolchain: stable, edição 2021, MSRV 1.75. Sem dependência nativa: `clap`, `serde`, `ureq`, `anyhow`, `ctrlc`.

## Estrutura do repositório

```
src/
  main.rs     dispatch, guard, janela de restauração
  cli.rs      superfície de argumentos (clap)
  ui.rs       tela inicial: banner, diagnóstico, menu
  detect.rs   sockets do Discord (ss / netstat)
  discord.rs  REST v10: membros, voice states, convites
  net.rs      iptables + tc (Linux) / firewall + QoS (Windows)
npm/
  lib.js      resolução de plataforma, download, verificação
  install.js  postinstall com fallback para cargo
  bin/        shim que executa o binário nativo
```

## Limites

- Ações de moderação são registradas no audit log do servidor: `drop`, `mute` e `deafen` ficam atribuídos ao token que as executou.
- `net block` depende de privilégio elevado. Sem ele, o comando informa e sai sem aplicar nada.
- A detecção lê sockets do próprio sistema: com Discord rodando em Flatpak ou sandbox, `ss` pode não expor o dono do socket.
- Token de usuário viola os termos do Discord. Use token de bot em servidor que você administra.

## Licença

MIT. Veja [LICENSE](LICENSE).

---

## English

**Silentium** is a voice kill switch for Discord, written in Rust. It detects an active call by inspecting the client's local UDP sockets (50000–65535), then drops, mutes, deafens or restores participants through the REST v10 API, or shapes the machine's network so only Discord traffic is cut.

```bash
npm install -g @onx90s/silentium
silentium            # banner, local diagnosis, interactive menu
silentium detect     # no credentials needed
```

Requires Node 18+ only for installation; the CLI itself is a native binary. Voice actions need a bot token with `MOVE_MEMBERS`, `MUTE_MEMBERS`, `DEAFEN_MEMBERS`. Network shaping needs root on Linux (iptables/tc) or Administrator on Windows (firewall/QoS).

Star the repo if it saved a call.
