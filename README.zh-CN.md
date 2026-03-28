**中文** | [English](README.md)

# BitFun

仅供中软内部试用，由软件IDE实验室提供技术支持。

## 构建

**前置依赖：**

- Node.js（推荐 LTS）
- pnpm
- Rust 工具链（rustup）
- Tauri 桌面端构建所需系统依赖（见 Tauri 官方文档「前置条件」）

**Windows 说明：** 桌面端使用预编译 OpenSSL。首次需要时会由 `ensure-openssl-windows.mjs` 将所需文件下载到 **`.bitfun/cache/`**。`pnpm run desktop:dev` 与 `pnpm run desktop:build*` 会调用该脚本；若仅使用 `cargo` 编译，请先在仓库根目录执行 **`node scripts/ensure-openssl-windows.mjs`**。也可设置 `BITFUN_SKIP_OPENSSL_BOOTSTRAP=1` 并自行配置 `OPENSSL_*`。

```bash
pnpm install
pnpm run desktop:dev
pnpm run desktop:build
```

### Linux

```bash
# Debian/Ubuntu 示例
sudo apt install libwebkit2gtk-4.1-dev build-essential libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf

pnpm install
pnpm run desktop:build:linux
```

产物位于 `src/apps/desktop/target/release/bundle/`（`.deb`、`.rpm`、`.AppImage` 等）。

其他发行版可参考仓库内 `docs/linux-setup.md`。
