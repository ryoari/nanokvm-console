#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null || { echo 'Install Rust with Cargo first.' >&2; exit 1; }
command -v lake >/dev/null || { echo 'Install elan and add its bin directory to PATH first.' >&2; exit 1; }
cargo build --release --locked
prefix=${PREFIX:-"$HOME/.local"}
# Versioned pairs prevent a partial upgrade from mixing the guard and server.
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)
build=$(sha256sum target/release/nanokvm-console | cut -d ' ' -f 1)
folder="$prefix/lib/nanokvm-console/$build"
mkdir -p "$folder" "$prefix/bin"
install -m 755 target/release/nanokvm-console "$folder/nanokvm-console"
install -m 755 verification/.lake/build/bin/console_guard "$folder/console_guard"
"$folder/nanokvm-console" --verify
ln -sfn "$folder/nanokvm-console" "$prefix/bin/nanokvm-console"
printf 'Installed %s/bin/nanokvm-console\n' "$prefix"
