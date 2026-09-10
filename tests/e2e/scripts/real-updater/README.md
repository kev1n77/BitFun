# Real BitFun Windows updater test

This builds the complete BitFun desktop application from v0.2.19 (base commit
1456c29092570a1174e8a880546793235a79eb0e). It uses the existing
`desktop:build:nsis:fast` entry point, all normal desktop features and resources,
and the original frontend, update dialog and Rust update commands. No replacement
application or updater implementation is used.

The two builds differ only in their projected application version (0.2.19 and
0.2.20). Both compile the origin repository's isolated test feed and existing
signing public key. The existing release-fast/devtools configuration enables the
product's embedded WebDriver for evidence collection in CI; it starts only when
the test environment explicitly supplies a loopback port.

Push this directory or its workflow on `codex/updater-real-windows-test` to run.
The CI creates two prereleases, `updater-real-20260910-0.2.19` and
`updater-real-20260910-0.2.20`, and preserves the existing Latest release.
It first installs 0.2.19 and proves no update, then publishes 0.2.20, restarts
0.2.19, captures the original notification dialog, clicks its actual install
button, and checks the automatically relaunched 0.2.20 executable and frontend.
The 0.2.20 release contains the screenshots and validation report.

The visible release copy is owned by `update-notes.txt` and uses the intended
production announcement wording, with the origin release as its download link.
CI checks that exact text in the real native response and original dialog.
When only test scripts or remote notes change, CI reuses the existing signed
packages after checking product-source equality, package hash, key fingerprint,
identity and channel. Set `BITFUN_REUSE_TEST_PACKAGES` to `0` to force a rebuild.

Manual check: install the first release's NSIS `.exe`, launch BitFun, and wait
for its usual update dialog (or use About -> Check for updates). The release
notes carry a simulated 1.0.0 download notice. The original dialog renders that
URL as plain text; no new link button has been added. Installing the offered
update runs the real 0.2.19 -> 0.2.20 updater path.

These are full BitFun test packages with the original app identity. Manual
installation therefore shares the normal BitFun installation/profile; use a
Windows test account or VM if an existing install must remain untouched. CI
uses a clean runner and the repository's existing isolated E2E storage.
No AI model is invoked. Remote workspace, remote control, peer mode, detached
dispatch, Linux/macOS, and migration to a real 1.x release are not exercised.

Focused verification (the runner must first have the corresponding real app open):

```powershell
node --check tests/e2e/scripts/real-updater/verify.mjs
node tests/e2e/scripts/real-updater/verify.mjs before tests/e2e/.bitfun/real-updater/out/evidence
```
