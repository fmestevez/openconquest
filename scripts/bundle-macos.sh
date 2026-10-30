#!/bin/sh
# Builds dist/OpenConquest.app, a double-clickable macOS application. On first
# start it asks for the Ancient Conquest CD (or a mounted image of it) or
# install folder, and remembers it.
set -e
cd "$(dirname "$0")/.."
# OPENCONQUEST_BIN: a game binary built already (the release's universal
# one); else it is built here for this Mac.
bin=${OPENCONQUEST_BIN:-target/release/openconquest}
if [ -z "${OPENCONQUEST_BIN:-}" ]; then
    cargo build --release -p openconquest
fi
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/OpenConquest.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/openconquest"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenConquest</string>
  <key>CFBundleDisplayName</key><string>OpenConquest</string>
  <key>CFBundleIdentifier</key><string>org.openconquest.OpenConquest</string>
  <key>CFBundleExecutable</key><string>openconquest</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.strategy-games</string>
  <key>CFBundleIconFile</key><string>OpenConquest</string>
</dict>
</plist>
PLIST
# The icon: the warrior of the original's title screen, taken from your
# game data (it is not part of OpenConquest), if the data can be found.
config="$HOME/Library/Application Support/OpenConquest/config.json"
for dir in "${OPENCONQUEST_DATA:-}" gamedata \
    "$(sed -n 's/.*"data": *"\(.*\)".*/\1/p' "$config" 2>/dev/null)" \
    "$HOME/Library/Application Support/OpenConquest/gamedata"; do
    if [ -n "$dir" ] && [ -f "$dir/controlm.dkx" ]; then data=$dir; break; fi
done
if [ -n "${data:-}" ]; then
    tmp=$(mktemp -d)
    cargo run -q --release -p openconquest-extract -- dump "$data/controlm.dkx" "$tmp" --filter 0M_BKGND_00 >/dev/null
    art=$(ls "$tmp"/0M_BKGND_00*.png | head -1)
    sips -c 300 300 --cropOffset 125 165 "$art" --out "$tmp/icon.png" >/dev/null
    mkdir -p "$tmp/OpenConquest.iconset"
    for size in 16 32 64 128 256 512; do
        sips -z $size $size "$tmp/icon.png" --out "$tmp/OpenConquest.iconset/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        sips -z $double $double "$tmp/icon.png" --out "$tmp/OpenConquest.iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$tmp/OpenConquest.iconset" -o "$app/Contents/Resources/OpenConquest.icns"
    rm -rf "$tmp"
else
    echo "no game data found: the app has no icon (set OPENCONQUEST_DATA to add one)"
fi
# Ad-hoc signed, so macOS runs it on this machine without complaint.
codesign --force --deep -s - "$app" >/dev/null 2>&1 || true
echo "Built $app"
