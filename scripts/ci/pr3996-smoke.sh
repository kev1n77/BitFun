#!/usr/bin/env bash
set -euo pipefail

print_startup_diagnostics() {
  local work_dir=$1
  tail -35 "$work_dir/startup.log" >&2
  find "$work_dir/logs" -type f -name '*.log' -print0 | while IFS= read -r -d '' log_file; do
    printf '\n--- %s ---\n' "$log_file" >&2
    grep -Ei -C 4 'panic|undefined symbol|error while loading shared libraries|fatal|segmentation fault|WebKit.*terminated' "$log_file" >&2 || true
    tail -24 "$log_file" >&2
  done
  find "$work_dir/logs" -type f -name 'crash-report.json' -print0 | while IFS= read -r -d '' crash_report; do
    jq '{location, message, thread_name, arch}' "$crash_report" >&2
  done
  if [[ -f "$work_dir/windows.txt" ]]; then tail -20 "$work_dir/windows.txt" >&2; fi
}

test_startup() {
  local work_dir=$1
  local program=$2
  local app_pid app_window deadline window_state stable_since=-1 app_exit=0
  mkdir -p "$work_dir"/{home,config,cache,data,runtime,app,logs}
  chmod 700 "$work_dir/runtime"
  export HOME="$work_dir/home" XDG_CONFIG_HOME="$work_dir/config" XDG_CACHE_HOME="$work_dir/cache"
  export XDG_DATA_HOME="$work_dir/data" XDG_RUNTIME_DIR="$work_dir/runtime" OPENBITFUN_HOME="$work_dir/app"
  export GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1 OPENBITFUN_LOG_DIR="$work_dir/logs"
  "$program" > "$work_dir/startup.log" 2>&1 &
  app_pid=$!
  trap "kill $app_pid 2>/dev/null || true; wait $app_pid 2>/dev/null || true" EXIT
  deadline=$((SECONDS + 180))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
      wait "$app_pid" || app_exit=$?
      printf "Desktop process exited with status %s\n" "$app_exit" >&2
      print_startup_diagnostics "$work_dir"
      return 1
    fi
    xwininfo -root -tree > "$work_dir/windows.txt"
    app_window=""
    while read -r candidate; do
      if window_state=$(xwininfo -id "$candidate" 2>/dev/null) && [[ "$window_state" == *'Map State: IsViewable'* ]]; then
        app_window=$candidate
        break
      fi
    done < <(awk 'tolower($0) ~ /openbitfun/ && $1 ~ /^0x/ && $0 ~ /[5-9][0-9][0-9]x[3-9][0-9][0-9]|[1-9][0-9][0-9][0-9]x[3-9][0-9][0-9]/ {print $1}' "$work_dir/windows.txt")
    if [[ -n "$app_window" ]]; then
      if (( stable_since < 0 )); then stable_since=$SECONDS; fi
      if (( SECONDS - stable_since >= 10 )); then
        awk -v id="$app_window" '$1 == id' "$work_dir/windows.txt"
        printf 'PASS: main window stably visible (%s)\n' "$program"
        return 0
      fi
    else
      stable_since=-1
    fi
    sleep 2
  done
  print_startup_diagnostics "$work_dir"
  return 1
}


start_weston() {
  local dir=$1
  local attempt
  sudo env DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=2 -o Acquire::http::Timeout=25 -y --no-install-recommends install weston xwayland
  mkdir -p "$dir"
  chmod 700 "$dir"
  XDG_RUNTIME_DIR="$dir" dbus-run-session -- weston --backend=headless-backend.so --width=1280 --height=900 --use-pixman --xwayland --socket=openbitfun-smoke --log="$dir/weston.log" &
  weston_pid=$!
  trap 'kill "$weston_pid" 2>/dev/null || true; wait "$weston_pid" 2>/dev/null || true' EXIT
  local xdisplay=''
  for attempt in $(seq 1 25); do
    if [[ -s "$dir/weston.log" ]]; then
      xdisplay=$(sed -nE 's/.*xserver listening on display (:[0-9]+).*/\1/p' "$dir/weston.log" | head -1)
      if [[ -n "$xdisplay" ]] && DISPLAY="$xdisplay" xwininfo -root >/dev/null 2>&1; then
        printf 'Testing X11 applications on Weston XWayland display %s\n' "$xdisplay"
        export DISPLAY="$xdisplay"
        return 0
      fi
    fi
    sleep 1
  done
  cat "$dir/weston.log" >&2
  return 1
}

desktop_startup() {
  if [[ "${USE_WESTON:-}" == 1 ]]; then
    dbus-run-session -- "$@"
  else
    xvfb-run -a -s '-screen 0 1280x900x24 -nolisten tcp' dbus-run-session -- "$@"
  fi
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
    deb_dependencies=$(dpkg-deb -f "$deb" Depends)
    printf 'DEB declared dependencies: %s\n' "$deb_dependencies"
    grep -Eq '(^|,)[[:space:]]*libayatana-appindicator3-1[[:space:]]*(,|$)' <<< "$deb_dependencies"
    sudo env DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=2 -o Acquire::http::Timeout=25 -y --no-install-recommends install libgles2
    if ldconfig -p | grep 'libayatana-appindicator3\.so\.1'; then
      echo 'AppImage verification requires a host without a tray runtime' >&2
      exit 1
    fi
    printf 'AppImage host has no Ayatana tray runtime\n'
    chmod +x "$artifact"
    appimage_ok=0
    deb_ok=0
    if [[ "$(uname -m)" == aarch64 && "$(lsb_release -rs)" == 22.04 ]]; then
      USE_WESTON=1
      start_weston "$root/weston"
      for attempt in 1 2 3; do
        if desktop_startup bash "$script" startup "$root/appimage-$attempt" "$artifact"; then
          echo "PASS: Ubuntu 22.04 ARM64 AppImage XWayland attempt $attempt"
          ((appimage_ok+=1))
        fi
      done
      cat "$root/weston/weston.log" | tail -40
      if [[ "$appimage_ok" == 3 ]]; then appimage_ok=1; else appimage_ok=0; fi
    elif desktop_startup bash "$script" startup "$root/appimage" "$artifact"; then
      appimage_ok=1
    fi
    if sudo env DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=2 -o Acquire::http::Timeout=25 -y install "$deb" > "$root/deb-install.log" 2>&1; then
      pkg=$(dpkg-deb -f "$deb" Package)
      dpkg-query -W -f='${Status} ${Package} ${Version} ${Architecture}\n' "$pkg" libayatana-appindicator3-1
      test "$(dpkg-query -W -f='${Status}' libayatana-appindicator3-1)" = 'install ok installed'
      if desktop_startup bash "$script" startup "$root/deb" /usr/bin/openbitfun-desktop; then deb_ok=1; fi
    else
      tail -35 "$root/deb-install.log" >&2
    fi
    printf 'RESULT appimage_startup=%d deb_install_and_startup=%d\n' "$appimage_ok" "$deb_ok"
    [[ "$appimage_ok" == 1 && "$deb_ok" == 1 ]]
    ;;
  *) exit 2 ;;
esac
