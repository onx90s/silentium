// rust — src/interrupt.rs — Ctrl-C compartilhado entre comandos de longa duração
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();

/// Instala o handler uma única vez; chamadas seguintes reutilizam o mesmo.
pub fn install() -> Result<(), String> {
    INSTALLED
        .get_or_init(|| {
            ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::SeqCst))
                .map_err(|err| err.to_string())
        })
        .clone()
}

/// Zera o sinal antes de começar um novo ciclo de espera.
pub fn reset() {
    INTERRUPTED.store(false, Ordering::SeqCst);
}

pub fn hit() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::{hit, install, reset};

    #[test]
    fn instalacao_e_idempotente() {
        assert!(install().is_ok(), "primeira instalação");
        assert!(install().is_ok(), "segunda instalação reutiliza o handler");
    }

    #[test]
    fn reset_limpa_o_sinal() {
        reset();
        assert!(!hit());
        reset();
        assert!(!hit());
    }
}
