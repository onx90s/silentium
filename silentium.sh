#!/usr/bin/env bash
# bash — silentium.sh — acha, baixa se preciso, e executa o silentium
set -euo pipefail

DEST="$HOME/.local/bin"
EXE="$DEST/silentium"
URL="https://github.com/onx90s/silentium/releases/latest/download/silentium-linux-x64.tar.gz"

for candidato in "$(dirname "$0")/silentium" "$(dirname "$0")/target/release/silentium" "$HOME/.cargo/bin/silentium"; do
    if [[ -x "$candidato" ]]; then
        exec "$candidato" "$@"
    fi
done

if [[ -x "$EXE" ]] && ! "$EXE" --version 2>/dev/null | grep -q .; then
    : # binário quebrado, baixa de novo
elif [[ -x "$EXE" ]]; then
    exec "$EXE" "$@"
fi

echo "[*] baixando o binário (~1.8 MB)..."
mkdir -p "$DEST"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$URL" -o "$tmp/silentium.tar.gz"
tar -xzf "$tmp/silentium.tar.gz" -C "$tmp"
install -m 755 "$tmp/silentium" "$EXE"
echo "[ok] instalado em $EXE"
exec "$EXE" "$@"
