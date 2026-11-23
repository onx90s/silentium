// rust — src/cli.rs — superfície de argumentos
use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "silentium",
    version,
    about = "Kill switch de voz para call do Discord",
    long_about = "Detecta a call ativa pelos sockets locais do Discord e derruba o tráfego\n\
                  dele por firewall (só o Discord, resto do PC intacto). Também desc,\n\
                  muta e ensurdece participantes via REST quando há token de bot com\n\
                  MOVE_MEMBERS / MUTE_MEMBERS / DEAFEN_MEMBERS."
)]
pub struct Cli {
    /// Token de bot, ou de usuário com --user-token (sem prefixo "Bot").
    #[arg(long, env = "DISCORD_TOKEN", global = true)]
    pub token: Option<String>,

    /// Trata o token como token de usuário (sem prefixo "Bot ").
    #[arg(long, global = true)]
    pub user_token: bool,

    /// ID do servidor. Pode ser omitido quando --invite é usado.
    #[arg(long, env = "GUILD_ID", global = true)]
    pub guild: Option<String>,

    /// Intervalo entre chamadas REST, em ms.
    #[arg(long, global = true, default_value_t = 150)]
    pub delay_ms: u64,

    /// Mostra o que faria, sem executar.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Sem subcomando, abre a tela inicial interativa.
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Cmd {
    /// Valida o token e mostra a conta do bot.
    Whoami,
    /// Resolve um convite (discord.gg/xxx) em guild + canal.
    Resolve { invite: String },
    /// Detecta a call ativa olhando os sockets locais do Discord. Não usa API.
    Detect,
    /// Verifica e instala a atualização mais recente publicada no GitHub.
    Update {
        /// Só informa se existe versão nova, sem instalar.
        #[arg(long)]
        check: bool,
    },
    /// Operações na sessão de voz.
    #[command(subcommand)]
    Voice(VoiceCmd),
    /// Shaping de rede no tráfego de voz.
    #[command(subcommand)]
    Net(NetCmd),
}

#[derive(Args, Debug, Clone, Default)]
pub struct TargetArgs {
    /// Restringe a um canal de voz específico.
    #[arg(long)]
    pub channel: Option<String>,
    /// Alvo específico (repetível). Sem isso, todos em voz.
    #[arg(long = "user")]
    pub user: Vec<String>,
    /// Convite da call; extrai guild e canal dele.
    #[arg(long)]
    pub invite: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum VoiceCmd {
    /// Quem está em voz agora.
    List(TargetArgs),
    /// Desconecta da voz (channel_id: null).
    Drop(TargetArgs),
    /// Muta no servidor.
    Mute(TargetArgs),
    /// Desmuta.
    Unmute(TargetArgs),
    /// Ensurdece no servidor.
    Deafen(TargetArgs),
    /// Remove deafen.
    Undeafen(TargetArgs),
    /// Desfaz mute e deafen de todos.
    Restore(TargetArgs),
    /// Vigia o canal e derruba quem entrar.
    Guard {
        #[command(flatten)]
        target: TargetArgs,
        /// Ação aplicada a quem entrar: drop | mute | deafen.
        #[arg(long, default_value = "drop")]
        action: String,
        /// Intervalo de polling, em segundos.
        #[arg(long, default_value_t = 2)]
        interval: u64,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum NetCmd {
    /// Aplica shaping no UDP de voz (50000-65535).
    Throttle {
        /// Banda permitida para o áudio, em kbps. 8 degrada, 2 mata.
        #[arg(long, default_value_t = 8)]
        kbps: u64,
        /// Perda de pacotes, em %. Linux.
        #[arg(long)]
        loss: Option<u8>,
        /// Latência adicional, em ms. Linux.
        #[arg(long = "latency")]
        delay: Option<u32>,
        /// Duração em segundos. Sem isso, permanece até `net clear`.
        #[arg(long)]
        seconds: Option<u64>,
        /// Interface de saída. Autodetectada se omitida.
        #[arg(long)]
        iface: Option<String>,
    },
    /// Remove todo o shaping aplicado.
    Clear {
        #[arg(long)]
        iface: Option<String>,
    },
    /// Corta a internet SÓ do Discord (firewall por processo). Outros apps seguem normais.
    Block {
        /// Duração em segundos. Sem isso, fica até Enter/Ctrl-C.
        #[arg(long)]
        seconds: Option<u64>,
    },
    /// Desfaz o bloqueio.
    Unblock,
    /// Detecta a call ativa e corta o Discord por N segundos.
    Auto {
        /// Duração em segundos. Sem isso, fica até Enter/Ctrl-C.
        #[arg(long)]
        seconds: Option<u64>,
        /// Corta mesmo se nenhuma call for detectada.
        #[arg(long)]
        force: bool,
    },
}
