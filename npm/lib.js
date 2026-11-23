// javascript — npm/lib.js — plataforma, download do release e resolução do binário
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");
const https = require("node:https");
const { execFileSync } = require("node:child_process");

const REPO = "onx90s/silentium";
const PKG_ROOT = path.join(__dirname, "..");
const VENDOR = path.join(__dirname, "vendor");

function packageVersion() {
  const pkg = JSON.parse(fs.readFileSync(path.join(PKG_ROOT, "package.json"), "utf8"));
  return pkg.version;
}

function platformTag() {
  const platform = process.platform;
  const arch = process.arch;
  if (platform === "linux" && (arch === "x64" || arch === "arm64")) return { platform: "linux", arch };
  if (platform === "darwin" && (arch === "x64" || arch === "arm64")) return { platform: "darwin", arch };
  if (platform === "win32" && arch === "x64") return { platform: "windows", arch };
  return null;
}

function isMusl() {
  if (process.platform !== "linux") return false;
  try {
    const report = process.report && process.report.getReport();
    return Boolean(report) && !report.header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

function binaryName() {
  return process.platform === "win32" ? "silentium.exe" : "silentium";
}

function assetName(tag) {
  const ext = tag.platform === "windows" ? "zip" : "tar.gz";
  return `silentium-${tag.platform}-${tag.arch}.${ext}`;
}

function vendorBinary() {
  return path.join(VENDOR, binaryName());
}

function localBuildBinary() {
  const name = binaryName();
  const candidates = [
    path.join(PKG_ROOT, "target", "release", name),
    path.join(PKG_ROOT, "..", "target", "release", name),
  ];
  return candidates.find((p) => fs.existsSync(p)) || null;
}

// Ordem: SILENTIUM_BINARY -> vendor/ -> build local do cargo.
function resolveBinary() {
  const override = process.env.SILENTIUM_BINARY;
  if (override && fs.existsSync(override)) return override;
  const vendored = vendorBinary();
  if (fs.existsSync(vendored)) return vendored;
  const built = localBuildBinary();
  if (built) return built;
  return null;
}

function download(url, destination, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > 5) return reject(new Error("redirects demais"));
    https
      .get(url, { headers: { "user-agent": `silentium-npm/${packageVersion()}` } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          return resolve(download(res.headers.location, destination, redirects + 1));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode} em ${url}`));
        }
        const file = fs.createWriteStream(destination);
        res.pipe(file);
        file.on("finish", () => file.close(() => resolve(destination)));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

async function fetchText(url) {
  const tmp = path.join(os.tmpdir(), `silentium-${process.pid}-${Date.now()}.txt`);
  await download(url, tmp);
  const text = fs.readFileSync(tmp, "utf8");
  fs.rmSync(tmp, { force: true });
  return text;
}

function sha256(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function extract(archive, tag, into) {
  if (tag.platform === "windows") {
    execFileSync(
      "powershell",
      ["-NoProfile", "-NonInteractive", "-Command", `Expand-Archive -Path '${archive}' -DestinationPath '${into}' -Force`],
      { stdio: "inherit" }
    );
  } else {
    execFileSync("tar", ["-xzf", archive, "-C", into], { stdio: "inherit" });
  }
}

async function downloadRelease(tag) {
  const version = packageVersion();
  const base = `https://github.com/${REPO}/releases/download/v${version}`;
  const asset = assetName(tag);
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "silentium-"));
  const archive = path.join(tmpDir, asset);

  await download(`${base}/${asset}`, archive);

  let expected = null;
  try {
    const sums = await fetchText(`${base}/SHASUMS256.txt`);
    const line = sums.split("\n").find((l) => l.trim().endsWith(asset));
    if (line) expected = line.trim().split(/\s+/)[0];
  } catch {
    // release sem checksums: segue, mas avisa
  }

  if (expected) {
    const actual = sha256(archive);
    if (actual !== expected) {
      fs.rmSync(tmpDir, { recursive: true, force: true });
      throw new Error(`checksum não bate: esperado ${expected}, obtido ${actual}`);
    }
  } else {
    console.warn("[silentium] aviso: SHASUMS256.txt ausente no release, pulando verificação");
  }

  extract(archive, tag, tmpDir);
  const candidates = [
    path.join(tmpDir, binaryName()),
    path.join(tmpDir, "silentium", binaryName()),
  ];
  const found = candidates.find((p) => fs.existsSync(p));
  if (!found) {
    fs.rmSync(tmpDir, { recursive: true, force: true });
    throw new Error("binário não encontrado dentro do arquivo baixado");
  }

  fs.mkdirSync(VENDOR, { recursive: true });
  const target = vendorBinary();
  fs.copyFileSync(found, target);
  if (process.platform !== "win32") fs.chmodSync(target, 0o755);
  fs.rmSync(tmpDir, { recursive: true, force: true });
  return target;
}

function buildFromSource() {
  const cargoToml = path.join(PKG_ROOT, "Cargo.toml");
  if (!fs.existsSync(cargoToml)) {
    throw new Error("sem Cargo.toml: este pacote não traz código-fonte para compilar");
  }
  execFileSync("cargo", ["build", "--release", "--locked"], {
    cwd: PKG_ROOT,
    stdio: "inherit",
  });
  const built = localBuildBinary();
  if (!built) throw new Error("cargo terminou sem produzir o binário");
  fs.mkdirSync(VENDOR, { recursive: true });
  const target = vendorBinary();
  fs.copyFileSync(built, target);
  if (process.platform !== "win32") fs.chmodSync(target, 0o755);
  return target;
}

// Instala o binário se ainda não estiver presente: release -> cargo.
async function ensureBinary({ quiet = false } = {}) {
  const existing = resolveBinary();
  if (existing) return existing;

  const tag = platformTag();
  if (tag && !isMusl()) {
    try {
      const target = await downloadRelease(tag);
      if (!quiet) console.log(`[silentium] binário instalado: ${target}`);
      return target;
    } catch (err) {
      if (!quiet) console.warn(`[silentium] download falhou (${err.message}); tentando compilar`);
    }
  } else if (!quiet) {
    console.warn(`[silentium] sem binário pronto para ${process.platform}/${process.arch}; compilando`);
  }

  const target = buildFromSource();
  if (!quiet) console.log(`[silentium] binário compilado: ${target}`);
  return target;
}

module.exports = {
  ensureBinary,
  REPO,
  PKG_ROOT,
  VENDOR,
  packageVersion,
  platformTag,
  assetName,
  binaryName,
  vendorBinary,
  resolveBinary,
  downloadRelease,
  buildFromSource,
  isMusl,
};
