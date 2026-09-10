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
It first installs 0.2.19 and proves no update, then publishes 0.2.20 and proves
that 0.2.19 still sees no update until its channel is promoted. It then restarts
0.2.19, captures the original notification dialog, clicks its actual install
button, and checks the automatically relaunched 0.2.20 executable and frontend.
The 0.2.20 release contains the screenshots and validation report.
The test brings the installed window forward using the existing host command.
It records window visibility, the auto-update setting, and the dialog trigger.
If the startup dialog does not appear, it exercises About -> Check for updates
and reports that manual trigger explicitly; it does not count that as proof of
an automatic startup notification. After installation, it independently records
the installed executable version and automatically started process. If the
Windows shell relaunch drops the test-only environment, the report explicitly
records the additional restart needed to attach WebDriver for the final UI check.

The two test Git tags are prepared from the authorized origin maintainer
checkout before CI starts. CI checks them before compiling, then uses its
GITHUB_TOKEN to publish releases/assets for those existing tags. This avoids
asking the workflow token to create tags across imported workflow history.
A recovered signed package may be staged in a draft release; CI publishes it
before running the installed application's public-feed checks.
The initial signed 0.2.19 build is also recovered from run 34470093395 while
that diagnostic artifact is retained. Missing recovery artifacts fall back to
the normal full build; package verification or UI failures remain fatal.

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

Verified on 2026-09-10 in [run 34485336504](https://github.com/kev1n77/BitFun/actions/runs/34485336504):
the frozen feed kept 0.2.19 unchanged; after promotion settled, the original
startup dialog displayed the exact announcement automatically. Its original
install button upgraded the installed executable to 0.2.20 and a new BitFun
process started automatically. The shell-started process did not expose the
test WebDriver endpoint, so the verifier restarted that installed application
with the E2E environment for its final native-version and frontend checks.
The release's `validation.json` records both observations separately. Temporary
IPC diagnostic instrumentation was removed after this run; it produced no
usable trace and was not part of the pass assertions.

Focused verification (the runner must first have the corresponding real app open):

```powershell
node --check tests/e2e/scripts/real-updater/verify.mjs
node tests/e2e/scripts/real-updater/verify.mjs before tests/e2e/.bitfun/real-updater/out/evidence
```
