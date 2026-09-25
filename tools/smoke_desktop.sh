#!/usr/bin/env bash
set -euo pipefail

test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
export XDG_DATA_HOME="$test_dir"
export LIBGL_ALWAYS_SOFTWARE=1
export WINIT_UNIX_BACKEND=x11
export WGPU_BACKEND=gl

xvfb-run -a -s '-screen 0 1280x800x24' bash -euo pipefail -c '
  ./target/debug/music-library > "$XDG_DATA_HOME/app.log" 2>&1 &
  app_pid=$!
  cleanup() {
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  }
  trap cleanup EXIT

  for attempt in {1..100}; do
    if xdotool search --onlyvisible --name "^Music Library$" >/dev/null 2>&1; then
      sleep 2
      if kill -0 "$app_pid" 2>/dev/null && test -s "$XDG_DATA_HOME/MusicLibrary/library.sqlite3"; then
        exit 0
      fi
      break
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      break
    fi
    sleep 0.2
  done

  cat "$XDG_DATA_HOME/app.log"
  exit 1
'
