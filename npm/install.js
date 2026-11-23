// javascript — npm/install.js — postinstall best-effort: nunca derruba a instalação
"use strict";

const lib = require("./lib.js");

async function main() {
  if (process.env.SILENTIUM_SKIP_DOWNLOAD === "1") {
    console.log("[silentium] SILENTIUM_SKIP_DOWNLOAD=1: download pulado");
    return;
  }

  const existing = lib.resolveBinary();
  if (existing) {
    console.log(`[silentium] binário presente: ${existing}`);
    return;
  }

  try {
    await lib.ensureBinary();
  } catch (err) {
    // O shim do CLI tenta de novo no primeiro uso: aqui nunca falhamos o install.
    console.warn(`[silentium] aviso: ${err.message}`);
    console.warn("[silentium] o binário será baixado ou compilado no primeiro `silentium`");
    if (process.env.SILENTIUM_STRICT === "1") throw err;
  }
}

main().catch((err) => {
  console.warn(`[silentium] aviso: ${err.message}`);
  if (process.env.SILENTIUM_STRICT === "1") process.exit(1);
});
