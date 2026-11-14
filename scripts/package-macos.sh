#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
app_dir="$project_dir/dist/Wyckoscope.app"

cd "$project_dir"
cargo build --release
mkdir -p "$app_dir/Contents/MacOS"
mkdir -p "$app_dir/Contents/Resources"
cp "$project_dir/target/release/wyckoscope" "$app_dir/Contents/MacOS/wyckoscope"
cp "$project_dir/packaging/macos/Info.plist" "$app_dir/Contents/Info.plist"

echo "Created $app_dir"

