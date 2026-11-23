// javascript — npm/install.js — postinstall: baixa o binário do release, com fallback para cargo
"use strict";

const lib = require("./lib.js");

async function main() {
  if (process.env.SILENTIUM_SKIP_DOWNLOAD === "1") {
    console.log("[silentium] SILENTIUM_SKIP_DOWNLOAD=1: instalação do binário pulada");
    return;
  }

  if (lib.resolveBinary()) {
    console.log(`[silentium] binário já presente: ${lib.resolveBinary()}`);
    return;
  }

  const tag = lib.platformTag();
  if (!tag) {
    console.warn(
      `[silentium] plataforma sem binário pronto (${process.platform}/${process.arch}); tentando compilar`
    );
  } else if (lib.isMusl()) {
    console.warn("[silentium] libc musl detectada; o binário do release é glibc, tentando compilar");
  } else {
    try {
      const target = await lib.downloadRelease(tag);
      console.log(`[silentium] binário instalado: ${target}`);
      return;
    } catch (err) {
      console.warn(`[silentium] download falhou (${err.message}); tentando compilar`);
    }
  }

  try {
    const target = lib.buildFromSource();
    console.log(`[silentium] binário compilado e instalado: ${target}`);
    return;
  } catch (err) {
    console.warn(`[silentium] compilação falhou: ${err.message}`);
  }

  console.warn("");
  console.warn("[silentium] não consegui instalar o binário automaticamente.");
  console.warn("[silentium] opções:");
  console.warn(`  - baixe de https://github.com/${lib.REPO}/releases e aponte SILENTIUM_BINARY`);
  console.warn("  - instale Rust (https://rustup.rs) e rode: cargo install --path .");
  console.warn("  - no Linux: curl -fsSL .../silentium-linux-x64.tar.gz | tar -xz -C ~/.local/bin");
  console.warn("");
  if (process.env.SILENTIUM_STRICT === "1") process.exit(1);
}

main().catch((err) => {
  console.warn(`[silentium] erro inesperado: ${err.message}`);
  if (process.env.SILENTIUM_STRICT === "1") process.exit(1);
});
