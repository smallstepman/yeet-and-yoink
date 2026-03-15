# yeet-and-yoink Chrome bridge

This repository contains the Chromium-family extension bridge for `yeet-and-yoink` plus a
per-user native-messaging host manifest template.

## Contents

- `manifest.json` and `service_worker.js`: the unpacked extension source tree.
- `install-native-host.sh`: installs a user-scoped native host manifest for the `yny` binary.
- `native-host/com.yeet_and_yoink.chromium_bridge.json.template`: template used by the installer.

## Install the extension

Open `chrome://extensions`, enable developer mode, and load this repository root as an unpacked
extension.

The embedded extension key keeps the extension ID stable across Chromium-family browsers that honor
it, which is required for native-messaging permissions.

## Install the native host manifest

```sh
./install-native-host.sh /absolute/path/to/yny [chromium|chrome|brave|edge]
```

Defaults:

- Linux Chromium: `~/.config/chromium/NativeMessagingHosts/`
- Linux Chrome: `~/.config/google-chrome/NativeMessagingHosts/`
- Linux Brave: `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/`
- Linux Edge: `~/.config/microsoft-edge/NativeMessagingHosts/`
- macOS browser-specific `~/Library/Application Support/.../NativeMessagingHosts/`

## Native host details

- Native host name: `com.yeet_and_yoink.chromium_bridge`
- Extension ID: `oigofebnnajpegmncnciacecfhlokkbp`
