#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
mkdir -p "$root/target"
(
  cd "$root/editors/vscode"
  npx --yes @vscode/vsce package --no-dependencies -o "$root/target/artscript.vsix"
)

app_cli() {
  for candidate in "$@"; do
    if [ -x "$candidate" ]; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

cursor_cli() {
  if app_cli \
    "/Applications/Cursor.app/Contents/Resources/app/bin/cursor" \
    "$HOME/Applications/Cursor.app/Contents/Resources/app/bin/cursor"
  then
    return 0
  fi
  if command -v cursor >/dev/null 2>&1; then
    command -v cursor
    return 0
  fi
  return 1
}

code_cli() {
  if app_cli \
    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" \
    "$HOME/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
  then
    return 0
  fi
  if command -v code >/dev/null 2>&1; then
    command -v code
    return 0
  fi
  return 1
}

installed=
if cli=$(cursor_cli); then
  "$cli" --install-extension "$root/target/artscript.vsix" --force
  installed="cursor"
fi
if cli=$(code_cli); then
  "$cli" --install-extension "$root/target/artscript.vsix" --force
  installed="${installed:+$installed }code"
fi
if [ -z "$installed" ]; then
  echo "no Cursor or VS Code CLI found" >&2
  exit 1
fi
echo "installed for $installed"
echo "reload the window"
