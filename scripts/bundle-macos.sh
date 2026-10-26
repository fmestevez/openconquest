#!/bin/sh
# Builds dist/OpenAC.app, a double-clickable macOS application. On first
# start it asks for the Ancient Conquest CD (or a mounted image of it) or
# install folder, and remembers it.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p openac
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/OpenAC.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/openac "$app/Contents/MacOS/openac"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenAC</string>
  <key>CFBundleDisplayName</key><string>OpenAC</string>
  <key>CFBundleIdentifier</key><string>org.openac.OpenAC</string>
  <key>CFBundleExecutable</key><string>openac</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.strategy-games</string>
  <key>CFBundleIconFile</key><string>OpenAC</string>
</dict>
</plist>
PLIST
# The icon: the warrior of the original's title screen, taken from your
# game data (it is not part of OpenAC), if the data can be found.
config="$HOME/Library/Application Support/OpenAC/config.json"
for dir in "${OPENAC_DATA:-}" gamedata \
    "$(sed -n 's/.*"data": *"\(.*\)".*/\1/p' "$config" 2>/dev/null)" \
    "$HOME/Library/Application Support/OpenAC/gamedata"; do
    if [ -n "$dir" ] && [ -f "$dir/controlm.dkx" ]; then data=$dir; break; fi
done
if [ -n "${data:-}" ]; then
    tmp=$(mktemp -d)
    cargo run -q --release -p openac-extract -- dump "$data/controlm.dkx" "$tmp" --filter 0M_BKGND_00 >/dev/null
    art=$(ls "$tmp"/0M_BKGND_00*.png | head -1)
    sips -c 300 300 --cropOffset 125 165 "$art" --out "$tmp/icon.png" >/dev/null
    mkdir -p "$tmp/OpenAC.iconset"
    for size in 16 32 64 128 256 512; do
        sips -z $size $size "$tmp/icon.png" --out "$tmp/OpenAC.iconset/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        sips -z $double $double "$tmp/icon.png" --out "$tmp/OpenAC.iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$tmp/OpenAC.iconset" -o "$app/Contents/Resources/OpenAC.icns"
    rm -rf "$tmp"
else
    echo "no game data found: the app has no icon (set OPENAC_DATA to add one)"
fi
# Ad-hoc signed, so macOS runs it on this machine without complaint.
codesign --force --deep -s - "$app" >/dev/null 2>&1 || true
echo "Built $app"
