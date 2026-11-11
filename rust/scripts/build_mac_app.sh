#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
  echo "ZirconRust.app can only be built on macOS." >&2
  exit 1
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
if [ "${1:-}" = "--release" ]; then
  cargo build -p mir-client --release
  profile=release
else
  cargo build -p mir-client
  profile=debug
fi

app="$root/target/$profile/ZirconRust.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/$profile/mir-client" "$app/Contents/MacOS/ZirconRust"
cat > "$app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.zirconrust.game</string>
  <key>CFBundleName</key><string>ZirconRust</string>
  <key>CFBundleDisplayName</key><string>ZirconRust</string>
  <key>CFBundleExecutable</key><string>ZirconRust</string>
  <key>CFBundleIconFile</key><string>ZirconRust.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
</dict></plist>
EOF

cp "$root/icons/Zircon.icns" "$app/Contents/Resources/ZirconRust.icns"
codesign --force --sign - "$app"
echo "Built $app"
