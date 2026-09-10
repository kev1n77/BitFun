# Windows updater channel lab

This is a small real Tauri application for testing the release protocol used by
BitFun v0.2.19. It is not a build of the full BitFun application. It pins Tauri
2.11.5 and tauri-plugin-updater 2.10.1, matching that release's Cargo.lock.

The lab builds actual NSIS packages with application versions 0.2.19 and 0.2.20.
Both use the existing origin repository's updater signing key. Nothing generates,
rotates, downloads, or prints a private key.

## Run

Push changes to `codex/updater-channel-windows-test` in `kev1n77/BitFun`. The
`Windows Updater Channel Lab` workflow runs directly from that branch; the
default branch does not need to be changed. It installs only the Tauri CLI and
the small standalone Rust application, not the root pnpm or agent workspace.

The repository already supplies these Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- `TAURI_UPDATER_PUBKEY`

The workflow creates or refreshes only two explicitly named test prereleases:

- `updater-lab-20260910-0.2.19`: receiver installer and two channel manifests.
- `updater-lab-20260910-0.2.20`: notification installer and validation evidence.

Both are marked prerelease and `latest=false`. Publications use the workflow's
GITHUB_TOKEN, preventing the repository's full release workflow from being
triggered recursively. No real release tag, default branch, production updater
feed, website mirror, CLI, or Relay deployment is modified.

## What is tested

1. Build and sign 0.2.19, publish both lab channels with the 0.2.19 manifest.
2. Install 0.2.19 into a dedicated runner temporary directory and check the
   public GitHub channel: no update is available.
3. Build and sign 0.2.20 with the same key. Its `notes` contains a Chinese
   simulated 1.0.0 announcement and a link to the second test release.
4. Publish 0.2.20 and replace only `channel-legacy.json` on the receiver release.
5. The installed 0.2.19 client discovers 0.2.20 and its notice. Checking
   `channel-control.json` still reports no update, despite using the same key.
6. Download through Tauri's real updater, verify minisign and compare SHA-256
   against the signed CI artifact.
7. Invoke Tauri's installer from the installed receiver. Confirm that NSIS
   replaces the executable and relaunches the real 0.2.20 app, with the same
   embedded public-key fingerprint and no remaining update.
8. Verify that the repository's existing Latest release did not change.

`validation.json` is uploaded to the second release. The workflow's diagnostic
artifact contains individual reports and both installers. An initial cold Rust
build is required; later runs reuse the standalone application cache.

## Manual Windows test

1. Download `UpdaterLab_0.2.19_windows-x86_64-setup.exe` from the first test release.
2. Install and open **OpenBitFun Updater Lab**. Windows x64 and WebView2 are required.
3. Leave the notification channel selected. The app automatically checks it and
   shows the Chinese notice plus the demonstration download-page URL.
4. Select the frozen control channel and check again: it reports no update.
5. Switch back, download and verify the package, then explicitly choose
   **Install test update and restart**. The restarted app should show 0.2.20.

The app identifier is `com.openbitfun.updaterlab.20260910`; installer identity,
registry entries, and app data are separate from BitFun and OpenBitFun. The lab
can be removed through Windows Installed apps after testing.

The notice is read from `latest.json.notes` as plain text, matching the old
client. The lab additionally provides an explicit download-page button; the
unmodified BitFun 0.2.19 dialog does not have that button. This experiment does
not prove full-product data migration, old package installation compatibility,
Linux/macOS behavior, or any remote-workspace/control/peer/dispatch behavior.

For the eventual product rollout, use dedicated 1.x feeds and leave old feeds
on the compatible bridge version. This test deliberately uses neither the
real `/releases/latest/` updater nor the website's `/release/latest.json`.
