#!/usr/bin/env bash
set -euo pipefail

test_startup() {
  local work_dir=$1
  local program=$2
  local app_pid app_window deadline
  mkdir -p "$work_dir"/{home,config,cache,data,runtime,app}
  chmod 700 "$work_dir/runtime"
  export HOME="$work_dir/home" XDG_CONFIG_HOME="$work_dir/config" XDG_CACHE_HOME="$work_dir/cache"
  export XDG_DATA_HOME="$work_dir/data" XDG_RUNTIME_DIR="$work_dir/runtime" OPENBITFUN_HOME="$work_dir/app"
  export GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1
  "$program" > "$work_dir/startup.log" 2>&1 &
  app_pid=$!
  trap "kill $app_pid 2>/dev/null || true; wait $app_pid 2>/dev/null || true" EXIT
  deadline=$((SECONDS + 180))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
      wait "$app_pid" || true
      tail -35 "$work_dir/startup.log" >&2
      return 1
    fi
    xwininfo -root -tree > "$work_dir/windows.txt"
    app_window=$(awk 'tolower($0) ~ /openbitfun/ && $1 ~ /^0x/ && $0 ~ /[5-9][0-9][0-9]x[3-9][0-9][0-9]|[1-9][0-9][0-9][0-9]x[3-9][0-9][0-9]/ {print $1; exit}' "$work_dir/windows.txt")
    if [[ -n "$app_window" ]] && xwininfo -id "$app_window" | grep -q 'Map State: IsViewable'; then
      sleep 10
      kill -0 "$app_pid" 2>/dev/null
      xwininfo -id "$app_window" | grep 'Map State: IsViewable'
      awk -v id="$app_window" '$1 == id' "$work_dir/windows.txt"
      printf 'PASS: main window stayed visible for 10 seconds (%s)\n' "$program"
      return 0
    fi
    sleep 2
  done
  tail -35 "$work_dir/startup.log" >&2
  cat "$work_dir/windows.txt" >&2
  return 1
}

case "${1:-}" in
  startup) test_startup "$2" "$3" ;;
  verify)
    root=$(realpath "$2")
    artifact=$(realpath "$3")
    deb=$(realpath "$4")
    script=$(realpath "$0")
    cat /etc/os-release | grep -E '^(PRETTY_NAME|VERSION_ID)='
    printf 'Architecture: %s\n' "$(uname -m)"
    printf 'glibc: %s\n' "$(getconf GNU_LIBC_VERSION)"
    chmod +x "$artifact"
    appimage_ok=0
    deb_ok=0
    if xvfb-run -a -s '-screen 0 1280x900x24 -nolisten tcp' dbus-run-session -- bash "$script" startup "$root/appimage" "$artifact"; then appimage_ok=1; fi
    if sudo env DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=2 -y install "$deb" > "$root/deb-install.log" 2>&1; then
      pkg=$(dpkg-deb -f "$deb" Package)
      dpkg-query -W -f='${Status} ${Package} ${Version} ${Architecture}\n' "$pkg"
      if xvfb-run -a -s '-screen 0 1280x900x24 -nolisten tcp' dbus-run-session -- bash "$script" startup "$root/deb" /usr/bin/openbitfun-desktop; then deb_ok=1; fi
    else
      tail -35 "$root/deb-install.log" >&2
    fi
    printf 'RESULT appimage_startup=%d deb_install_and_startup=%d\n' "$appimage_ok" "$deb_ok"
    [[ "$appimage_ok" == 1 && "$deb_ok" == 1 ]]
    ;;
  *) exit 2 ;;
esac
