# Windows updater channel lab

This is an isolated test application, not a product host. It uses the Tauri and
updater versions from BitFun v0.2.19 without compiling the agent workspace.

- Only the dedicated test branch and `updater-lab-*` prereleases may be published.
- Use the existing repository signing secrets; never print or export private keys.
- Keep the application identifier and installation directory separate from BitFun.
- Keep both signed package versions and validate their actual installed versions.
- The control channel must remain unchanged when the notification channel advances.

Focused verification:

```powershell
node --test tests/updater-channel-lab/configure.test.mjs
cargo fmt --manifest-path tests/updater-channel-lab/src-tauri/Cargo.toml -- --check
```

The `Windows Updater Channel Lab` workflow builds and installs both Windows
versions, checks both public channels with the installed receiver, verifies the
download signature, and exercises the updater-driven installation and relaunch.
Do not run the full product build for this fixture.
