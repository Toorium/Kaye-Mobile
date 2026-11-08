# Publishes a new Kaye for Android on GitHub, so every installed copy offers it with "New update".
#
#   powershell -ExecutionPolicy Bypass -File scripts\publish-apk.ps1 -Notes "What's new, in a line or two"
#
# The version is Cargo.toml's ([workspace.package]), which is the computer version this was
# built from, so the phone and computer releases share numbers. Commit everything first. Then
# this:
#   1. builds dist\Kaye-<version>.apk with build-apk.ps1
#   2. pushes main to Kaye-Mobile, and makes the GitHub release v<version> there from that
#      commit, with the .apk
#   3. writes latest-android.json with the .apk's size, SHA-256 and link, then commits and
#      pushes it
#
# latest-android.json goes last, so nobody is offered an update whose .apk isn't up yet. If the
# upload fails, the half-made release is deleted again. It signs in to GitHub the way git push
# does, through Git Credential Manager.

param(
    [Parameter(Mandatory = $true)][string]$Notes,
    [string]$Repository = 'Toorium/Kaye-Mobile'
)

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
$latestPath = Join-Path $project 'latest-android.json'

function Invoke-Git {
    & git -C $project @args
    if ($LASTEXITCODE -ne 0) { throw "git $args failed" }
}

$version = (Select-String -LiteralPath (Join-Path $project 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$published = '0.0.0'
if (Test-Path -LiteralPath $latestPath) {
    $published = (Get-Content -LiteralPath $latestPath -Raw | ConvertFrom-Json).version
}
if ([version]$version -le [version]$published) {
    throw "Cargo.toml says $version, and $published is already published for Android. Raise the version in Cargo.toml first."
}
if (git -C $project status --porcelain) {
    throw 'Commit everything first, so the release is made from exactly what gets built.'
}
if ((git -C $project rev-parse --abbrev-ref HEAD).Trim() -ne 'main') {
    throw 'Publish from main, which is where Kaye reads latest-android.json.'
}
$origin = (git -C $project remote get-url origin).Trim()
if ($origin -notmatch 'Kaye-Mobile') {
    throw "origin is $origin, not Kaye-Mobile. Phone releases only go to Kaye-Mobile."
}

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
# Some shells pipe text to programs with a UTF-8 byte order mark first, which git then reads as
# part of "protocol" and refuses. Plain UTF-8 is used for this one pipe, then put back.
$plain = New-Object Text.UTF8Encoding $false
$before = $OutputEncoding, [Console]::InputEncoding
$OutputEncoding = $plain
try { [Console]::InputEncoding = $plain } catch {}
$credential = "protocol=https`nhost=github.com`n`n" | git credential fill
$OutputEncoding = $before[0]
try { [Console]::InputEncoding = $before[1] } catch {}
$token = ($credential | Where-Object { $_ -like 'password=*' } | Select-Object -First 1) -replace '^password=', ''
if (-not $token) { throw 'No GitHub sign-in was found. Run git push once and sign in, then try again.' }
$headers = @{
    Authorization          = "Bearer $token"
    Accept                 = 'application/vnd.github+json'
    'X-GitHub-Api-Version' = '2022-11-28'
    'User-Agent'           = 'Kaye-publish'
}

& (Join-Path $PSScriptRoot 'build-apk.ps1')
$apk = Join-Path $project "dist\Kaye-$version.apk"
if (-not (Test-Path -LiteralPath $apk)) { throw "$apk wasn't built." }

Invoke-Git push origin main
$commit = (git -C $project rev-parse HEAD).Trim()
$tag = "v$version"
$made = Invoke-RestMethod -Method Post -Uri "https://api.github.com/repos/$Repository/releases" -Headers $headers -ContentType 'application/json' `
    -Body (@{ tag_name = $tag; target_commitish = $commit; name = "Kaye $version for Android"; body = $Notes } | ConvertTo-Json)

try {
    $name = "Kaye-$version.apk"
    $uri = "https://uploads.github.com/repos/$Repository/releases/$($made.id)/assets?name=$([uri]::EscapeDataString($name))"
    $asset = Invoke-RestMethod -Method Post -Uri $uri -Headers $headers -InFile $apk -ContentType 'application/vnd.android.package-archive'
} catch {
    Write-Host 'The upload failed, so the release is being taken down again.'
    Invoke-RestMethod -Method Delete -Uri "https://api.github.com/repos/$Repository/releases/$($made.id)" -Headers $headers | Out-Null
    Invoke-RestMethod -Method Delete -Uri "https://api.github.com/repos/$Repository/git/refs/tags/$tag" -Headers $headers | Out-Null
    throw
}

$file = [ordered]@{
    path   = 'Kaye.apk'
    size   = (Get-Item -LiteralPath $apk).Length
    sha256 = (Get-FileHash -LiteralPath $apk -Algorithm SHA256).Hash.ToLowerInvariant()
    url    = $asset.browser_download_url
}
$latest = [ordered]@{ version = $version; notes = $Notes; files = @($file) }
[IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 4) + "`n", (New-Object Text.UTF8Encoding $false))
Invoke-Git add latest-android.json
Invoke-Git commit -m "Publish Kaye $version for Android"
Invoke-Git push origin main
Write-Host "Published Kaye $version for Android at $($made.html_url)"
Write-Host "The .apk: $($asset.browser_download_url)"
Write-Host 'Installed copies offer it on their next launch. GitHub can take up to 5 minutes to serve the new latest-android.json.'
