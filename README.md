# yeet-and-yoink WezTerm plugin

This repository is laid out as a native WezTerm plugin repository.

## Install from a local checkout

```lua
local wezterm = require 'wezterm'
local yny = wezterm.plugin.require('file:///absolute/path/to/yeet-and-yoink/plugins/wezterm')
local config = wezterm.config_builder()

yny.apply_to_config(config)

return config
```

## Install from GitHub

Clone the plugin branch by itself and point WezTerm at that checkout:

```sh
git clone --branch plugin/wezterm --single-branch https://github.com/smallstepman/yeet-and-yoink.git yny-wezterm
```

Then use the local-checkout snippet above with `file:///absolute/path/to/yny-wezterm`.

The plugin entrypoint lives at `plugin/init.lua`, which is the structure WezTerm expects when
loading plugins via `wezterm.plugin.require(...)`.
