#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
  printf 'usage: %s /absolute/path/to/yny [chromium|chrome|brave|edge] [manifest-dir]\n' "$0" >&2
  exit 64
fi

yny_path=$1
browser=${2:-chromium}
override_dir=${3:-}

case "$yny_path" in
  /*) ;;
  *)
    printf 'yny path must be absolute\n' >&2
    exit 64
    ;;
esac

if [ -n "$override_dir" ]; then
  target_dir=$override_dir
else
  case "$(uname -s):$browser" in
    Linux:chromium)
      target_dir="$HOME/.config/chromium/NativeMessagingHosts"
      ;;
    Linux:chrome|Linux:google-chrome)
      target_dir="$HOME/.config/google-chrome/NativeMessagingHosts"
      ;;
    Linux:brave)
      target_dir="$HOME/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"
      ;;
    Linux:edge)
      target_dir="$HOME/.config/microsoft-edge/NativeMessagingHosts"
      ;;
    Darwin:chromium)
      target_dir="$HOME/Library/Application Support/Chromium/NativeMessagingHosts"
      ;;
    Darwin:chrome|Darwin:google-chrome)
      target_dir="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
      ;;
    Darwin:brave)
      target_dir="$HOME/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"
      ;;
    Darwin:edge)
      target_dir="$HOME/Library/Application Support/Microsoft Edge/NativeMessagingHosts"
      ;;
    *)
      printf 'unsupported browser/platform combination; pass manifest-dir explicitly\n' >&2
      exit 64
      ;;
  esac
fi

mkdir -p "$target_dir"
manifest_path="$target_dir/com.yeet_and_yoink.chromium_bridge.json"
python3 - "$yny_path" "$manifest_path" <<'PY'
import pathlib
import sys

yny_path = sys.argv[1]
out_path = pathlib.Path(sys.argv[2])
template = pathlib.Path('native-host/com.yeet_and_yoink.chromium_bridge.json.template').read_text(encoding='utf-8')
out_path.write_text(template.replace('__YNY_BINARY__', yny_path), encoding='utf-8')
PY
printf 'Wrote %s\n' "$manifest_path"
