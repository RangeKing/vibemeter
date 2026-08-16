#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 /absolute/path/to/VibeMeter.dmg" >&2
  exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "DMG finalization requires macOS." >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_dmg="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
if [[ ! -f "$source_dmg" || "$source_dmg" != *.dmg ]]; then
  echo "DMG not found: $source_dmg" >&2
  exit 2
fi

python3 -c 'import ds_store' 2>/dev/null || {
  echo "Missing Python dependency. Run: python3 -m pip install -r $script_dir/requirements-dmg.txt" >&2
  exit 2
}

work_dir="$(mktemp -d /tmp/vibemeter-dmg-finalize.XXXXXX)"
writable_dmg="$work_dir/writable.dmg"
fixed_dmg="$work_dir/fixed.dmg"
attach_plist="$work_dir/attach.plist"
device=""

cleanup() {
  if [[ -n "$device" ]]; then
    hdiutil detach "$device" >/dev/null 2>&1 || true
  fi
  case "$work_dir" in
    /tmp/vibemeter-dmg-finalize.*) rm -rf -- "$work_dir" ;;
  esac
}
trap cleanup EXIT

hdiutil convert "$source_dmg" -format UDRW -o "$writable_dmg" >/dev/null
hdiutil attach -nobrowse -noautoopen -plist "$writable_dmg" > "$attach_plist"

device="$(python3 - "$attach_plist" <<'PY'
import plistlib
import sys
with open(sys.argv[1], "rb") as stream:
    entities = plistlib.load(stream)["system-entities"]
print(next(item["dev-entry"] for item in entities if "mount-point" in item))
PY
)"
mount_point="$(python3 - "$attach_plist" <<'PY'
import plistlib
import sys
with open(sys.argv[1], "rb") as stream:
    entities = plistlib.load(stream)["system-entities"]
print(next(item["mount-point"] for item in entities if "mount-point" in item))
PY
)"

python3 "$script_dir/dmg_layout.py" finalize "$mount_point"
for item in .background .VolumeIcon.icns; do
  /usr/bin/SetFile -a V "$mount_point/$item"
  chflags hidden "$mount_point/$item"
done
sync
hdiutil detach "$device" >/dev/null
device=""

hdiutil convert "$writable_dmg" -format UDZO -imagekey zlib-level=9 -o "$fixed_dmg" >/dev/null
mv -f "$fixed_dmg" "$source_dmg"
echo "Finalized DMG layout: $source_dmg"
