[中文](README.zh-CN.md) | **English**

# BitFun

For internal trial use within Central Software Institute only. Technical support is provided by IDE Lab.

## Build

**Prerequisites**

- Node.js (LTS recommended)
- pnpm
- Rust toolchain (via rustup)
- Tauri desktop prerequisites for your OS (see Tauri docs: *Prerequisites*)

**Windows:** The desktop build uses a **prebuilt** OpenSSL. On first need, `ensure-openssl-windows.mjs` downloads artifacts into **`.bitfun/cache/`**. `pnpm run desktop:dev` and `pnpm run desktop:build*` run this script; if you compile with `cargo` only, run **`node scripts/ensure-openssl-windows.mjs`** once from the repo root first. Alternatively set `BITFUN_SKIP_OPENSSL_BOOTSTRAP=1` and supply your own `OPENSSL_*`.

```bash
pnpm install
pnpm run desktop:dev
pnpm run desktop:build
```

### Linux

```bash
# Debian/Ubuntu example
sudo apt install libwebkit2gtk-4.1-dev build-essential libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf

pnpm install
pnpm run desktop:build:linux
```

Artifacts are under `src/apps/desktop/target/release/bundle/` (`.deb`, `.rpm`, `.AppImage`, etc.). For other distros, see `docs/linux-setup.md` in this repository.
