#!/usr/bin/env node
// javascript — npm/bin/silentium.js — executa o binário nativo, instalando sob demanda
"use strict";

const { spawn } = require("node:child_process");
const lib = require("../lib.js");

async function main() {
  let binary = lib.resolveBinary();

  if (!binary) {
    console.error("[silentium] primeira execução: preparando o binário…");
    try {
      binary = await lib.ensureBinary();
    } catch (err) {
      console.error(`[silentium] não consegui preparar o binário: ${err.message}`);
      console.error(`[silentium] releases: https://github.com/${lib.REPO}/releases`);
      console.error("[silentium] alternative: cargo install --git https://github.com/" + lib.REPO);
      process.exit(1);
    }
  }

  const child = spawn(binary, process.argv.slice(2), {
    stdio: "inherit",
    env: process.env,
  });

  child.on("error", (err) => {
    console.error(`[silentium] falha ao executar o binário: ${err.message}`);
    process.exit(1);
  });

  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, () => {
      if (!child.killed) child.kill(signal);
    });
  }

  child.on("exit", (code, signal) => {
    if (signal) process.kill(process.pid, signal);
    process.exit(code === null ? 1 : code);
  });
}

main();
