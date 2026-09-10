$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($env:GITHUB_REPOSITORY -ne 'kev1n77/BitFun') { throw 'This lab is restricted to the origin test repository.' }
if ($env:GITHUB_REF -ne 'refs/heads/codex/updater-channel-windows-test') { throw 'Unexpected test branch.' }
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -or -not $env:TAURI_UPDATER_PUBKEY) { throw 'Existing updater signing secrets are required.' }

$labDir = $PSScriptRoot
$outDir = Join-Path $labDir 'out'
$evidenceDir = Join-Path $outDir 'evidence'
$installDir = Join-Path $env:RUNNER_TEMP 'openbitfun-updater-lab-install'
$installedExe = Join-Path $installDir 'openbitfun-updater-lab.exe'
$repo = $env:GITHUB_REPOSITORY
$receiverTag = "$($env:LAB_TAG_PREFIX)-0.2.19"
$publisherTag = "$($env:LAB_TAG_PREFIX)-0.2.20"
New-Item -ItemType Directory -Path $outDir, $evidenceDir -Force | Out-Null

function Write-Json($Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 15) + "`n", [Text.UTF8Encoding]::new($false))
}

function Invoke-Gh([string[]]$Arguments) {
    $result = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw "GitHub CLI failed: $($Arguments[0..1] -join ' ')" }
    return $result
}

function Build-Package([string]$Version) {
    Write-Host "Building the real signed $Version Windows test package."
    & node (Join-Path $labDir 'configure.mjs') $Version | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Lab configuration failed.' }
    Push-Location $labDir
    try {
        & tauri build --ci --bundles nsis | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Tauri build failed for $Version." }
    } finally { Pop-Location }
    $bundleDir = Join-Path $labDir 'src-tauri/target/release/bundle/nsis'
    $installers = @(Get-ChildItem -LiteralPath $bundleDir -Filter "*_${Version}_*-setup.exe")
    if ($installers.Count -ne 1) { throw "Expected one $Version NSIS installer; found $($installers.Count)." }
    $source = $installers[0].FullName
    $assets = Join-Path $outDir $Version
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    $name = "UpdaterLab_${Version}_windows-x86_64-setup.exe"
    Copy-Item -LiteralPath $source -Destination (Join-Path $assets $name)
    Copy-Item -LiteralPath "$source.sig" -Destination (Join-Path $assets "$name.sig")
    $plan = Get-Content -Raw (Join-Path $outDir 'plan.json') | ConvertFrom-Json
    $notes = if ($Version -eq '0.2.20') {
        "NOTIFICATION_TEST_1_0`n【通知测试】OpenBitFun 1.0.0 需要单独下载安装。`n演示下载入口：$($plan.downloadPage)`n本包只是兼容的 0.2.20 测试应用，不会安装真正的 OpenBitFun 1.0.0。"
    } else { 'Receiver baseline: no update notification is expected before the second release.' }
    $manifest = @{
        version = $Version
        notes = $notes
        pub_date = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = @{
            'windows-x86_64' = @{
                url = "https://github.com/$repo/releases/download/$($plan.tag)/$name"
                signature = [IO.File]::ReadAllText("$source.sig").Trim()
            }
        }
    }
    Write-Json (Join-Path $assets 'latest.json') $manifest
    $hash = (Get-FileHash -LiteralPath (Join-Path $assets $name) -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $assets 'SHA256SUMS'), "$hash  $name`n")
    return @{ Version = $Version; Assets = $assets; Installer = (Join-Path $assets $name); Tag = $plan.tag; Sha256 = $hash }
}

function Publish-LabRelease($Package, [string]$Description) {
    $tag = $Package.Tag
    if (-not $tag.StartsWith('updater-lab-')) { throw 'Refusing to publish a product release.' }
    $bodyPath = Join-Path $Package.Assets 'release-notes.md'
    $body = @"
$Description

This is an isolated Windows updater experiment, not a full BitFun release.
Application version: $($Package.Version). Tauri: 2.11.5. Updater: 2.10.1.
The existing repository updater key signs both versions; no key is rotated.

Install the 0.2.19 receiver to see the 0.2.20 notification.
Notification channel: https://github.com/$repo/releases/download/$receiverTag/channel-legacy.json
Frozen control channel: https://github.com/$repo/releases/download/$receiverTag/channel-control.json

The test application has its own identity and data directory. It requires Windows x64 and WebView2.
Both releases are prereleases and never replace the repository's Latest release.
"@
    [IO.File]::WriteAllText($bodyPath, $body, [Text.UTF8Encoding]::new($false))
    & gh release view $tag --repo $repo --json tagName 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Invoke-Gh @('release', 'edit', $tag, '--repo', $repo, '--prerelease', '--latest=false', '--notes-file', $bodyPath) | Out-Null
    } else {
        # GITHUB_TOKEN-created releases do not trigger the full desktop release workflow.
        Invoke-Gh @('release', 'create', $tag, '--repo', $repo, '--target', $env:GITHUB_SHA, '--prerelease', '--latest=false', '--title', "Updater Lab $($Package.Version) (Windows test)", '--notes-file', $bodyPath) | Out-Null
    }
    $uploads = @(Get-ChildItem -LiteralPath $Package.Assets -File | Where-Object { $_.Name -ne 'release-notes.md' } | ForEach-Object FullName)
    Invoke-Gh (@('release', 'upload', $tag, '--repo', $repo, '--clobber') + $uploads) | Out-Null
}

function Run-Receiver([string]$Name, [string[]]$Extra = @()) {
    $reportPath = Join-Path $evidenceDir "$Name.json"
    $arguments = @('--lab-smoke', '--report', "`"$reportPath`"") + $Extra
    $process = Start-Process -FilePath $installedExe -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(240000)) {
        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
        throw "Receiver smoke check timed out: $Name"
    }
    $process.Refresh()
    if (-not (Test-Path -LiteralPath $reportPath)) { throw "No smoke report from receiver: $Name, exit=$($process.ExitCode)" }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    if ($report.PSObject.Properties.Name -contains 'error') { throw "Receiver failed: $($report.error)" }
    if ($process.ExitCode -ne 0) { throw "Receiver failed: $Name, exit=$($process.ExitCode)" }
    return $report
}

function Await-Channel([bool]$ExpectUpdate, [string]$Name) {
    for ($attempt = 1; $attempt -le 10; $attempt++) {
        try {
            $report = Run-Receiver "$Name-$attempt"
            if ($report.updateAvailable -eq $ExpectUpdate -and (-not $ExpectUpdate -or $report.latestVersion -eq '0.2.20')) {
                return $report
            }
            Write-Host "Waiting for the public GitHub asset cache to expose the expected channel version (attempt $attempt)."
        } catch { Write-Host "Channel check attempt ${attempt}: $($_.Exception.Message)" }
        Start-Sleep -Seconds 10
    }
    throw 'The published channel did not reach the expected state.'
}

$productionLatestBefore = Invoke-Gh @('api', "repos/$repo/releases/latest", '--jq', '.tag_name')
$receiver = Build-Package '0.2.19'
Copy-Item -LiteralPath (Join-Path $receiver.Assets 'latest.json') -Destination (Join-Path $receiver.Assets 'channel-legacy.json')
Copy-Item -LiteralPath (Join-Path $receiver.Assets 'latest.json') -Destination (Join-Path $receiver.Assets 'channel-control.json')
Publish-LabRelease $receiver 'Receiver release: install this version first to receive the test notification.'

Write-Host 'Installing the receiver in the dedicated runner temporary directory.'
$installer = Start-Process -FilePath $receiver.Installer -ArgumentList @('/S', "/D=$installDir") -WindowStyle Hidden -PassThru
if (-not $installer.WaitForExit(120000)) { Stop-Process -Id $installer.Id; throw 'Receiver installation timed out.' }
$installer.Refresh()
if ($installer.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $installedExe)) { throw 'Receiver installation failed.' }
$before = Await-Channel $false 'before-publication'
if ($before.currentVersion -ne '0.2.19') { throw 'The installed receiver is not actually version 0.2.19.' }

$publisher = Build-Package '0.2.20'
Publish-LabRelease $publisher 'Notification release: the 0.2.20 manifest carries the simulated 1.0.0 download notice.'

# Change one feed only. The control channel continues serving the signed 0.2.19 package.
$promoted = Join-Path $outDir 'channel-legacy.json'
Copy-Item -LiteralPath (Join-Path $publisher.Assets 'latest.json') -Destination $promoted
Invoke-Gh @('release', 'upload', $receiverTag, '--repo', $repo, '--clobber', $promoted) | Out-Null

$notified = Await-Channel $true 'after-publication'
if ($notified.notes -notlike '*NOTIFICATION_TEST_1_0*' -or $notified.notes -notlike "*/releases/tag/$publisherTag*") {
    throw 'The old receiver did not receive the download-page notification.'
}
$control = Run-Receiver 'control-channel' @('--channel', 'control')
if ($control.updateAvailable -or $control.currentVersion -ne '0.2.19') { throw 'The frozen channel leaked the update.' }
$verified = Run-Receiver 'signature-verified' @('--verify-download')
if (-not $verified.downloadVerified -or $verified.packageSha256 -ne $publisher.Sha256) { throw 'The signed package download was not verified.' }
if ($before.publicKeySha256 -ne $verified.publicKeySha256) { throw 'The receiver trust root changed.' }

Write-Host 'Exercising updater-driven NSIS installation and automatic relaunch.'
$upgradeReport = Join-Path $evidenceDir 'after-install.json'
$upgrade = Start-Process -FilePath $installedExe -ArgumentList @('--lab-upgrade', '--report', "`"$upgradeReport`"") -WindowStyle Hidden -PassThru
if (-not $upgrade.WaitForExit(240000)) { Stop-Process -Id $upgrade.Id; throw 'Updater installation timed out.' }
for ($attempt = 1; $attempt -le 60 -and -not (Test-Path -LiteralPath $upgradeReport); $attempt++) { Start-Sleep -Seconds 2 }
if (-not (Test-Path -LiteralPath $upgradeReport)) { throw 'The updated application did not relaunch and write its report.' }
$after = Get-Content -LiteralPath $upgradeReport -Raw | ConvertFrom-Json
if ($after.PSObject.Properties.Name -contains 'error') { throw "Relaunched app failed: $($after.error)" }
if ($after.currentVersion -ne '0.2.20' -or $after.updateAvailable) { throw 'The installed publisher version did not settle at 0.2.20.' }
if ($after.publicKeySha256 -ne $before.publicKeySha256) { throw 'The two application versions contain different public keys.' }

$productionLatestAfter = Invoke-Gh @('api', "repos/$repo/releases/latest", '--jq', '.tag_name')
if ($productionLatestBefore -ne $productionLatestAfter) { throw 'The lab changed the production Latest release.' }
$evidence = @{
    result = 'passed'
    receiverBeforePublication = $before
    receiverAfterPublication = $notified
    frozenControl = $control
    verifiedDownload = $verified
    installedAndRelaunched = $after
    productionLatestBefore = $productionLatestBefore
    productionLatestAfter = $productionLatestAfter
    commit = $env:GITHUB_SHA
    runUrl = "https://github.com/$repo/actions/runs/$env:GITHUB_RUN_ID"
}
$validationPath = Join-Path $outDir 'validation.json'
Write-Json $validationPath $evidence
Invoke-Gh @('release', 'upload', $publisherTag, '--repo', $repo, '--clobber', $validationPath) | Out-Null

$summary = @"
## Windows updater channel experiment passed

| Check | Result |
| --- | --- |
| Installed receiver before publication | 0.2.19, no update |
| Receiver after channel promotion | 0.2.20 notification, download-page text present |
| Frozen control channel | No update |
| Same signing key | Verified by both embedded public-key fingerprints |
| Download integrity | Tauri minisign verification and SHA-256 match |
| Actual NSIS update and relaunch | 0.2.19 -> 0.2.20 |
| Existing Latest release | Unchanged: $productionLatestAfter |

[Receiver release](https://github.com/$repo/releases/tag/$receiverTag)
[Notification release](https://github.com/$repo/releases/tag/$publisherTag)
"@
[IO.File]::WriteAllText((Join-Path $outDir 'SUMMARY.md'), $summary)
Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value $summary
Write-Host 'All Windows updater channel checks passed.'
