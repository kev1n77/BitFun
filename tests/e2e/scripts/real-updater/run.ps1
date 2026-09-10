$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($env:GITHUB_REPOSITORY -ne 'kev1n77/BitFun' -or $env:GITHUB_REF -ne 'refs/heads/codex/updater-real-windows-test') {
    throw 'This experiment is restricted to its origin test branch.'
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -or -not $env:TAURI_UPDATER_PUBKEY) { throw 'Existing signing secrets are required.' }
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$runtime = Join-Path $root 'tests/e2e/.bitfun/real-updater'
$out = Join-Path $runtime 'out'
$evidenceDir = Join-Path $out 'evidence'
$installDir = Join-Path $env:RUNNER_TEMP 'real-bitfun-updater-install'
$installedExe = Join-Path $installDir 'bitfun-desktop.exe'
$repo = $env:GITHUB_REPOSITORY
$receiverTag = 'updater-real-20260910-0.2.19'
$publisherTag = 'updater-real-20260910-0.2.20'
New-Item -ItemType Directory -Path $out, $evidenceDir -Force | Out-Null

# The real product's existing E2E isolation; no product source is patched.
$env:BITFUN_WEBDRIVER_PORT = '4445'
$env:BITFUN_WEBDRIVER_LABEL = 'main'
$env:BITFUN_E2E_STORAGE_GUARD = '1'
$env:BITFUN_USER_ROOT = Join-Path $runtime 'user-root'
$env:BITFUN_E2E_USER_ROOT = $env:BITFUN_USER_ROOT
$env:BITFUN_HOME = Join-Path $runtime 'home'
$env:BITFUN_E2E_HOME = $env:BITFUN_HOME
$env:BITFUN_E2E_LOG_DIR = Join-Path $out 'logs'

function Write-Json($Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 20) + "`n", [Text.UTF8Encoding]::new($false))
}

function Invoke-Gh([string[]]$Arguments) {
    $result = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw "GitHub CLI failed: $($Arguments[0..1] -join ' ')" }
    return $result
}

function Build-Package([string]$Version) {
    Write-Host "Building the complete BitFun $Version desktop app and NSIS installer."
    & node scripts/set-build-version.mjs --version $Version | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Version projection failed.' }
    # Frontend API generation uses Cargo --locked before the desktop compile.
    # Synchronize the projected workspace package versions first, preserving
    # the already locked third-party dependency versions.
    & cargo update --workspace | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Workspace lockfile version projection failed.' }
    & node scripts/verify-release-version-sync.mjs --version $Version | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Version verification failed.' }
    # Existing full-product entry point; only the build profile and bundle target
    # are narrowed. All normal desktop features and frontend resources remain.
    & pnpm run desktop:build:nsis:fast | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Full BitFun $Version build failed." }
    $bundleDir = Join-Path $root 'target/release-fast/bundle/nsis'
    $installers = @(Get-ChildItem -LiteralPath $bundleDir -Filter "*_${Version}_*-setup.exe")
    if ($installers.Count -ne 1) { throw "Expected one real $Version NSIS installer; found $($installers.Count)." }
    $source = $installers[0].FullName
    $assets = Join-Path $out $Version
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    $name = "BitFun_${Version}_windows-x86_64-updater-test-setup.exe"
    $installer = Join-Path $assets $name
    Copy-Item -LiteralPath $source -Destination $installer
    Copy-Item -LiteralPath "$source.sig" -Destination "$installer.sig"
    $tag = if ($Version -eq '0.2.19') { $receiverTag } else { $publisherTag }
    $notes = if ($Version -eq '0.2.20') {
        (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'update-notes.txt') -Raw).Trim()
    } else { 'Real BitFun 0.2.19 receiver baseline; no update is available yet.' }
    Write-Json (Join-Path $assets 'latest.json') @{
        version = $Version
        notes = $notes
        pub_date = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = @{
            'windows-x86_64' = @{
                url = "https://github.com/$repo/releases/download/$tag/$name"
                signature = [IO.File]::ReadAllText("$source.sig").Trim()
            }
        }
    }
    $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $assets 'SHA256SUMS'), "$hash  $name`n")
    $generatedConfigs = @(Get-ChildItem (Join-Path $root 'src/apps/desktop/gen') -Filter 'tauri.*.generated.conf.json')
    if ($generatedConfigs.Count -ne 1) { throw 'Expected exactly one generated real desktop configuration.' }
    $config = Get-Content -LiteralPath $generatedConfigs[0].FullName -Raw | ConvertFrom-Json
    if ($config.productName -ne 'BitFun' -or $config.identifier -ne 'com.bitfun.desktop') { throw 'The build changed the real BitFun identity.' }
    foreach ($endpoint in $config.plugins.updater.endpoints) {
        if ($endpoint -ne $env:TAURI_UPDATER_ENDPOINT) { throw 'An updater endpoint escaped the dedicated test channel.' }
    }
    $keyBytes = [Text.Encoding]::UTF8.GetBytes($config.plugins.updater.pubkey.Trim())
    $keyHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($keyBytes)).ToLowerInvariant()
    $metadata = @{
        version = $Version; packageSha256 = $hash; publicKeySha256 = $keyHash
        baseSourceCommit = '1456c29092570a1174e8a880546793235a79eb0e'
        testCommit = $env:GITHUB_SHA; profile = 'release-fast'; productName = $config.productName
        identifier = $config.identifier; endpoints = $config.plugins.updater.endpoints
    }
    Write-Json (Join-Path $assets 'build-metadata.json') $metadata
    return @{ Version = $Version; Assets = $assets; Installer = $installer; Tag = $tag; Metadata = $metadata }
}

function Get-Package([string]$Version) {
    $tag = if ($Version -eq '0.2.19') { $receiverTag } else { $publisherTag }
    if ($env:BITFUN_REUSE_TEST_PACKAGES -ne '1') { return Build-Package $Version }
    & gh release view $tag --repo $repo --json tagName 2>$null | Out-Null
    $releaseExists = $LASTEXITCODE -eq 0
    # Release notes are remote metadata. Reuse the already built real binaries
    # only when all product source and packaging inputs are unchanged.
    $assets = Join-Path $out $Version
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    $name = "BitFun_${Version}_windows-x86_64-updater-test-setup.exe"
    if ($releaseExists) {
        Invoke-Gh @('release', 'download', $tag, '--repo', $repo, '--dir', $assets, '--clobber', '--pattern', $name, '--pattern', "$name.sig", '--pattern', 'build-metadata.json', '--pattern', 'latest.json', '--pattern', 'SHA256SUMS') | Out-Null
    } else {
        $recovered = Join-Path $root "tests/e2e/.bitfun/recovered-first-build/$Version"
        if (-not (Test-Path -LiteralPath (Join-Path $recovered 'build-metadata.json'))) { return Build-Package $Version }
        foreach ($file in @($name, "$name.sig", 'build-metadata.json', 'latest.json', 'SHA256SUMS')) {
            Copy-Item -LiteralPath (Join-Path $recovered $file) -Destination (Join-Path $assets $file)
        }
    }
    $metadata = Get-Content -LiteralPath (Join-Path $assets 'build-metadata.json') -Raw | ConvertFrom-Json
    if ($metadata.baseSourceCommit -ne '1456c29092570a1174e8a880546793235a79eb0e' -or $metadata.version -ne $Version) { throw 'Unexpected existing package provenance.' }
    & git diff --quiet $metadata.testCommit HEAD -- . ':!tests' ':!.github'
    if ($LASTEXITCODE -ne 0) { return Build-Package $Version }
    $installer = Join-Path $assets $name
    $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    $keyBytes = [Text.Encoding]::UTF8.GetBytes($env:TAURI_UPDATER_PUBKEY.Trim())
    $keyHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($keyBytes)).ToLowerInvariant()
    if ($metadata.packageSha256 -ne $hash -or $metadata.publicKeySha256 -ne $keyHash) { throw 'Existing package integrity or signing key mismatch.' }
    if ($metadata.productName -ne 'BitFun' -or $metadata.identifier -ne 'com.bitfun.desktop') { throw 'Existing package is not the real BitFun application.' }
    foreach ($endpoint in $metadata.endpoints) {
        if ($endpoint -ne $env:TAURI_UPDATER_ENDPOINT) { throw 'Existing package uses a different channel.' }
    }
    Write-Host "Reusing the verified real BitFun $Version binary; publishing updated remote release notes."
    return @{ Version = $Version; Assets = $assets; Installer = $installer; Tag = $tag; Metadata = $metadata }
}

function Publish-Package($Package) {
    if ($Package.Tag -notin @($receiverTag, $publisherTag)) { throw 'Unexpected test release tag.' }
    $manifestPath = Join-Path $Package.Assets 'latest.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($Package.Version -eq '0.2.20') {
        $manifest.notes = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'update-notes.txt') -Raw).Trim()
    }
    Write-Json $manifestPath $manifest
    $notesFile = Join-Path $Package.Assets 'release-notes.md'
    $notes = @"
Complete real BitFun $($Package.Version) Windows updater test package.

Built from the real v0.2.19 product source, using the existing release-fast NSIS
build. All original frontend, backend, update dialog and update commands remain.
The 0.2.20 build projects the version forward for this controlled experiment.
Both builds use the existing repository signing key and an isolated test feed.

Install the 0.2.19 receiver and wait for the original BitFun update dialog, or
use About -> Check for updates. The notes contain a simulated 1.0.0 notice.
The URL is plain text, as in the original application. The normal install button
updates to this experiment's real 0.2.20 BitFun package.

This retains the normal BitFun installation/profile identity. For manual tests,
use a Windows test account or VM if an existing BitFun install must be preserved.
CI uses a clean runner with isolated E2E data. This is not a production release.

Source and test instructions: https://github.com/$repo/tree/codex/updater-real-windows-test/tests/e2e/scripts/real-updater
CI: https://github.com/$repo/actions/runs/$env:GITHUB_RUN_ID
"@
    [IO.File]::WriteAllText($notesFile, $notes, [Text.UTF8Encoding]::new($false))
    & gh release view $Package.Tag --repo $repo --json tagName 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Invoke-Gh @('release', 'edit', $Package.Tag, '--repo', $repo, '--draft=false', '--prerelease', '--latest=false', '--notes-file', $notesFile) | Out-Null
    } else {
        Invoke-Gh @('release', 'create', $Package.Tag, '--repo', $repo, '--target', $env:GITHUB_SHA, '--prerelease', '--latest=false', '--title', "REAL BitFun $($Package.Version) - Windows updater test", '--notes-file', $notesFile) | Out-Null
    }
    $uploads = @(Get-ChildItem -LiteralPath $Package.Assets -File | Where-Object { $_.Name -ne 'release-notes.md' } | ForEach-Object FullName)
    Invoke-Gh (@('release', 'upload', $Package.Tag, '--repo', $repo, '--clobber') + $uploads) | Out-Null
}

function Start-BitFun {
    return Start-Process -FilePath $installedExe -WindowStyle Hidden -PassThru
}

function Verify-Phase([string]$Phase) {
    & node (Join-Path $PSScriptRoot 'verify.mjs') $Phase $evidenceDir | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Real BitFun $Phase verification failed." }
}

$productionBefore = Invoke-Gh @('api', "repos/$repo/releases/latest", '--jq', '.tag_name')
# Tag creation across imported workflow history can exceed GITHUB_TOKEN's
# capabilities. Tags are prepared using the authorized origin maintainer
# checkout; the workflow token only publishes these existing test tags.
foreach ($tag in @($receiverTag, $publisherTag)) {
    Invoke-Gh @('api', "repos/$repo/git/ref/tags/$tag") | Out-Null
}
Push-Location $root
try {
    $receiver = Get-Package '0.2.19'
    Copy-Item -LiteralPath (Join-Path $receiver.Assets 'latest.json') -Destination (Join-Path $receiver.Assets 'channel-legacy.json')
    Publish-Package $receiver
    Write-Host 'Installing the complete real 0.2.19 receiver on the clean runner.'
    $installer = Start-Process -FilePath $receiver.Installer -ArgumentList @('/S', "/D=$installDir") -WindowStyle Hidden -PassThru
    if (-not $installer.WaitForExit(300000)) { Stop-Process -Id $installer.Id; throw 'Real receiver installation timed out.' }
    $installer.Refresh()
    if ($installer.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $installedExe)) { throw 'Real receiver installation failed.' }
    $app = Start-BitFun
    try { Verify-Phase 'before' } finally { Stop-Process -Id $app.Id -ErrorAction SilentlyContinue }

    $publisher = Get-Package '0.2.20'
    if ($publisher.Metadata.publicKeySha256 -ne $receiver.Metadata.publicKeySha256) { throw 'Signing public keys differ.' }
    Publish-Package $publisher
    # A newer signed release alone must not move a client on a frozen feed.
    $app = Start-BitFun
    try { Verify-Phase 'isolated' } finally { Stop-Process -Id $app.Id -ErrorAction SilentlyContinue }
    $promoted = Join-Path $out 'channel-legacy.json'
    Copy-Item -LiteralPath (Join-Path $publisher.Assets 'latest.json') -Destination $promoted
    Invoke-Gh @('release', 'upload', $receiverTag, '--repo', $repo, '--clobber', $promoted) | Out-Null

    $app = Start-BitFun
    Verify-Phase 'notification'
    # The Node test clicked the shipped frontend button, which downloads,
    # verifies and installs through the unmodified production Rust command.
    if (-not $app.WaitForExit(600000)) { throw 'The real application did not exit to install its update.' }
    # NSIS may relaunch through the Windows shell without inheriting the
    # parent's test-only WebDriver environment. Observe the real installed
    # version and new process first, independently of that test endpoint.
    $restart = $null
    for ($attempt = 0; $attempt -lt 90; $attempt++) {
        $fileVersion = if (Test-Path -LiteralPath $installedExe) { (Get-Item -LiteralPath $installedExe).VersionInfo.ProductVersion } else { $null }
        $processes = @(Get-Process -Name 'bitfun-desktop' -ErrorAction SilentlyContinue | ForEach-Object {
            @{ id = $_.Id; path = $_.Path; title = $_.MainWindowTitle }
        })
        $restart = @{
            installedPath = $installedExe; installedVersion = $fileVersion
            previousProcessId = $app.Id; processes = $processes
            observedAt = [DateTime]::UtcNow.ToString('o')
        }
        Write-Json (Join-Path $evidenceDir 'installer-relaunch.json') $restart
        $restarted = @($processes | Where-Object { $_.path -eq $installedExe -and $_.id -ne $app.Id })
        if ($fileVersion -match '^0\.2\.20(?:\.|$)' -and $restarted.Count -gt 0) { break }
        Start-Sleep -Seconds 2
    }
    if ($restart.installedVersion -notmatch '^0\.2\.20(?:\.|$)' -or $restarted.Count -eq 0) {
        throw 'The original installer did not produce a running 0.2.20 application; see installer-relaunch.json.'
    }
    $restart.automaticRelaunchObserved = $true
    $restart.testDriverRestartRequired = $false
    Start-Sleep -Seconds 5
    try { Invoke-RestMethod 'http://127.0.0.1:4445/status' -TimeoutSec 5 | Out-Null }
    catch {
        $restart.testDriverRestartRequired = $true
        # The actual automatic relaunch is already recorded above. Restart
        # only that installed test process to reattach isolated E2E storage
        # and WebDriver for the final native/DOM checks.
        foreach ($process in $restarted) { Stop-Process -Id $process.id -ErrorAction SilentlyContinue }
        $app = Start-BitFun
    }
    Write-Json (Join-Path $evidenceDir 'installer-relaunch.json') $restart
    Verify-Phase 'after'
    $productionAfter = Invoke-Gh @('api', "repos/$repo/releases/latest", '--jq', '.tag_name')
    if ($productionAfter -ne $productionBefore) { throw 'The existing Latest release changed.' }
    Write-Json (Join-Path $out 'validation.json') @{
        result = 'passed'; application = 'real BitFun desktop'; originalUpdaterCodeUnmodified = $true
        receiverBuild = $receiver.Metadata; publisherBuild = $publisher.Metadata
        receiverBeforePublication = (Get-Content (Join-Path $evidenceDir 'before.json') -Raw | ConvertFrom-Json)
        receiverAfterReleaseBeforeChannelPromotion = (Get-Content (Join-Path $evidenceDir 'isolated.json') -Raw | ConvertFrom-Json)
        originalNotificationDialog = (Get-Content (Join-Path $evidenceDir 'notification.json') -Raw | ConvertFrom-Json)
        installerRelaunch = $restart
        installedApplication = (Get-Content (Join-Path $evidenceDir 'after.json') -Raw | ConvertFrom-Json)
        productionLatestBefore = $productionBefore; productionLatestAfter = $productionAfter
        runUrl = "https://github.com/$repo/actions/runs/$env:GITHUB_RUN_ID"
    }
    Invoke-Gh @('release', 'upload', $publisherTag, '--repo', $repo, '--clobber', (Join-Path $out 'validation.json'), (Join-Path $evidenceDir 'real-bitfun-notification.png'), (Join-Path $evidenceDir 'real-bitfun-after.png')) | Out-Null
    Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value "Real BitFun 0.2.19 -> original notification dialog -> original install button -> automatically relaunched BitFun 0.2.20: PASSED. Existing Latest: $productionAfter."
} finally { Pop-Location }
