**中文** | [English](AGENTS.md)

# AGENTS-CN.md

## 适用范围

本文件适用于 `BitFun-Installer`。仓库级规则请看顶层 `AGENTS.md`。

## 这里最重要的内容

`BitFun-Installer` 是独立的 Tauri + React 应用，不属于主 Cargo workspace。

模块 README 明确提到的重要区域：

- `src-tauri/src/installer/commands.rs`：Tauri IPC 与卸载执行
- `src-tauri/src/installer/registry.rs`：Windows 注册表集成
- `src-tauri/src/installer/shortcut.rs`：快捷方式创建
- `src-tauri/src/installer/extract.rs`：压缩包解压
- `src/hooks/useInstaller.ts`：前端安装流程状态

安装流程：

```text
Language Select → Options → Progress → Model Setup → Theme Setup
```

## 命令

```bash
pnpm --dir BitFun-Installer run installer:dev
pnpm --dir BitFun-Installer run tauri:dev
pnpm --dir BitFun-Installer run type-check
pnpm --dir BitFun-Installer run build
pnpm --dir BitFun-Installer run installer:build
```

## 验证

```bash
pnpm --dir BitFun-Installer run type-check && pnpm --dir BitFun-Installer run installer:build
```

如果修改了卸载流程，还需要验证 `BitFun-Installer/README.md` 中描述的卸载模式入口。
