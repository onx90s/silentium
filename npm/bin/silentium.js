#!/usr/bin/env node
// javascript — npm/bin/silentium.js — executar o binário nativo mantendo o terminal
"use strict";

const path = require("node:path");
const { spawn } = require("node:child_process");
const lib = require("../lib.js");

const binary = lib.resolveBinary();

if (!binary) {
  console.error("[silentium] binário não encontrado.");
  console.error("[silentium] rode: npm rebuild silentium   (ou reinstale: npm i -g silentium)");
  console.error(`[silentium] releases: https://github.com/${lib.REPO}/releases`);
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: false,
  env: process.env,
});

child.on("error", (err) => {
  console.error(`[silentium] falha ao executar ${path.basename(binary)}: ${err.message}`);
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
