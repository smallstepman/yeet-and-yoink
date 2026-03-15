# yeet-and-yoink kitty integration

Kitty does not need a standalone runtime plugin for `yeet-and-yoink`; it only needs remote control
exposed to detached callers. This repository packages that requirement as an includeable kitty
configuration snippet.

## Install

Either append this line to `~/.config/kitty/kitty.conf`:

```conf
include /absolute/path/to/yeet-and-yoink/plugins/kitty/yny.conf
```

Or run:

```sh
./install.sh
```

The installer adds an `include .../yny.conf` line to the target kitty config if it is not already
present.
