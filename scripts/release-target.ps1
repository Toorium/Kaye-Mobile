# Where build-installer.ps1 builds, for it and publish-update.ps1 to share. It goes beside
# .cargo\config.toml's target-dir when there is one, so it lands on the same drive as the
# rest of the build output, else under %LOCALAPPDATA%.

function Get-ReleaseTarget([string]$project) {
    $config = Join-Path $project '.cargo\config.toml'
    if (Test-Path -LiteralPath $config) {
        $line = Select-String -LiteralPath $config -Pattern '^target-dir\s*=\s*"(.+)"'
        if ($line) {
            return Join-Path (Split-Path -Parent $line.Matches[0].Groups[1].Value) 'kaye-release-target'
        }
    }
    Join-Path $env:LOCALAPPDATA 'kaye-release-target'
}
