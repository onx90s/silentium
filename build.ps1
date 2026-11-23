param([switch]$Test, [switch]$Install)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root

if ($Test) { cargo test --release }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build falhou" }

$exe = Join-Path $root 'target\release\silentium.exe'
Write-Host "[+] binário: $exe"

if ($Install) {
    $dest = Join-Path $env:LOCALAPPDATA 'Programs\Silentium'
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item $exe $dest -Force
    $userPath = [Environment]::GetEnvironmentVariable('Path','User')
    if ($userPath -notlike "*$dest*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$dest", 'User')
        Write-Host "[+] adicionado ao PATH do usuário: $dest"
    }
    Write-Host "[+] instalado em $dest\silentium.exe (reabra o terminal)"
}
