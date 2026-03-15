#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd -P)
target=${1:-$HOME/.config/kitty/kitty.conf}
include_line="include $script_dir/yny.conf"

mkdir -p "$(dirname "$target")"
touch "$target"

if grep -Fqx "$include_line" "$target"; then
  printf 'Already configured in %s\n' "$target"
  exit 0
fi

printf '\n%s\n' "$include_line" >> "$target"
printf 'Added %s to %s\n' "$include_line" "$target"
