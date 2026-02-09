[中文](README.zh-CN.md) | **English**

<div align="center">

![BitFun](./png/BitFun_title.png)

</div>
<div align="center">

[![GitHub release](https://img.shields.io/github/v/release/GCWing/BitFun?style=flat-square&color=blue)](https://github.com/GCWing/BitFun/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow?style=flat-square)](https://github.com/GCWing/BitFun/blob/main/LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-blue?style=flat-square)](https://github.com/GCWing/BitFun)

</div>

---

## A Word Up Front

In the age of AI, we believe human–machine collaboration needs a fundamentally new form of software. Today, the programming domain is the most mature starting point for that exploration.

## What Is BitFun

BitFun is an Agentic Development Environment (ADE). While featuring a cutting-edge Code Agent system, we are more committed to deeply exploring and defining human–machine collaboration patterns, built with Rust + TypeScript for an ultra-lightweight and fluid experience.

![BitFun](./png/first_screen_screenshot.png)


### Working Modes

| Mode | Scenario | Characteristics |
|------|----------|-----------------|
| **Agentic** | Day-to-day coding | Conversation-driven; AI autonomously reads, edits, runs, and verifies. |
| **Plan** | Complex tasks | Plan first, then execute; align on critical changes upfront. |
| **Debug** | Hard problems | Instrument & trace → compare paths → root-cause analysis → verify fix. |
| **Review** | Code review | Review code based on key repository conventions. |
---


### Extensibility

- **MCP Protocol**: Extend with external tools and resources via MCP servers.
- **Skills**: Markdown/script-based capability packages that teach the Agent specific tasks (auto-reads Cursor, Claude Code, Codex configs).
- **Agent Customization**: Quickly define specialized Agents with Markdown.
- **Rules**: Quickly customize professional Agents via Markdown (auto-reads Cursor configs).

---


## Quick Start

### Use Directly

Download the latest installer for the desktop app from [Release](https://github.com/GCWing/BitFun/releases). After installation, configure your model and you're ready to go.

Other form factors are currently only specification drafts and not yet developed. If needed, please build from source.

### Build from Source

Make sure you have the following prerequisites installed:

- Node.js (LTS recommended)
- pnpm (run `corepack enable`)
- Rust toolchain (install via [rustup](https://rustup.rs/))
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for desktop development

```bash
# Install dependencies
pnpm install

# Run desktop app in development mode
npm run desktop:dev

# Build desktop app
npm run desktop:build
```

For more details, see the [Contributing Guide](./CONTRIBUTING.md).

## Platform Support

The project uses a Rust + TypeScript tech stack, supporting cross-platform and multi-form-factor reuse.
| Form Factor | Supported Platforms | Status |
|-------------|---------------------|--------|
| **Desktop** (Tauri) | Windows, macOS | ✅ Supported |
| **CLI** | Windows, macOS, Linux | 🚧 In Development |
| **Server** | - | 🚧 In Development |
| **Mobile** | - | 🚧 In Development |



## Contributing
We welcome great ideas and code contributions. We are maximally accepting of AI-generated code. Please submit PRs to the dev branch first; we will periodically review and sync to the main branch.

Key contribution areas we focus on:
1. Contributing good ideas/creativity (features, interactions, visuals, etc.), submit issues
2. Optimizing the Agent system and its effectiveness
3. Improving system stability and foundational capabilities
4. Expanding the ecosystem (Skills, MCP, LSP plugins, or better support for specific vertical development scenarios)


## Disclaimer
1. This project is built in spare time for exploring and researching next-generation human–machine collaborative interaction, not for commercial profit.
2. 97%+ of this project was built with Vibe Coding. Feedback on code issues is also welcome—refactoring and optimization can be done via AI.
3. This project depends on and references many open-source projects. Thanks to all open-source authors. **If your rights are affected, please contact us for rectification.**

---
<div align="center">
The world is being rewritten—this time, we are all holding the pen.
</div>
