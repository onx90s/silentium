# Silentium

```
██████╗  ██╗ ██╗     ██████╗ ███╗   ███╗ ████████╗ ██╗ ██╗   ██╗ ███╗   ███╗
██╔═══╝  ██║ ██║     ██╔═══╝ ████╗ ████║ ╚══██╔══╝ ██║ ██║   ██║ ████╗ ████║
██████╗  ██║ ██║     █████╗  ██╔████╔██║    ██║    ██║ ██║   ██║ ██╔████╔██║
╚═══██║  ██║ ██║     ██╔══╝  ██║╚██╔╝██║    ██║    ██║ ██║   ██║ ██║╚██╔╝██║
██████║  ██║ ██████╗ ██████╗ ██║ ╚═╝ ██║    ██║    ██║ ╚██████╔╝ ██║ ╚═╝ ██║
╚═════╝  ╚═╝ ╚═════╝ ╚═════╝ ╚═╝     ╚═╝    ╚═╝    ╚═╝  ╚═════╝  ╚═╝     ╚═╝
```Kill switch de voz para Discord. Detecta a call pelos sockets locais do cliente e corta, limita, muta ou derruba — sem plugin, sem injeção.

[![release](https://img.shields.io/github/v/release/onx90s/silentium?color=6c8cff)](https://github.com/onx90s/silentium/releases)
[![npm](https://img.shields.io/npm/v/@onxdev/silentium?label=npm&color=cb3837)](https://www.npmjs.com/package/@onxdev/silentium)
[![ci](https://img.shields.io/github/actions/workflow/status/onx90s/silentium/ci.yml?label=ci)](https://github.com/onx90s/silentium/actions)
[![license](https://img.shields.io/badge/license-MIT-8fbcbb)](#licença)

## Instalar

```bash
npm install -g @onxdev/silentium     # baixa o binário do release
cargo install --git https://github.com/onx90s/silentium --locked
```

Binário direto: assets `silentium-linux-x64`, `-linux-arm64`, `-darwin-x64`, `-darwin-arm64`, `-windows-x64.zip` em [releases](https://github.com/onx90s/silentium/releases), com `SHASUMS256.txt`.

## Usar

```bash
silentium                            # tela inicial: diagnóstico + menu
silentium detect                     # acha a call ativa, sem token
silentium voice drop --invite discord.gg/abc123
silentium net auto --seconds 30      # corta o Discord por 30s
```

`net` corta só o processo do Discord — o resto da máquina fica intacto.

## Comandos

| | |
|---|---|
| `detect` | call ativa pelos sockets locais, zero credencial |
| `net auto` / `block` / `unblock` / `throttle` | corta, limita, restaura o tráfego de voz |
| `voice list` / `drop` / `mute` / `deafen` / `restore` / `guard` | controle de participantes via REST |
| `resolve` | convite → guild + canal |

Controle de voz exige token com `MOVE_MEMBERS`, `MUTE_MEMBERS` e `DEAFEN_MEMBERS`.

## Build

```bash
cargo build --release
./build.ps1 -Install        # Windows: copia para %LOCALAPPDATA%\Silentium e põe no PATH
```

MIT.
