# Builds Kaye for Android as dist\Kaye-<version>.apk, signed and ready to install.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-apk.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\build-apk.ps1 -Emulator   (also x86_64, for the emulator)
#
# It needs, all on this machine:
#   - a tools folder: E:\tools if there is one, else %USERPROFILE%\tools
#   - the Android SDK in it as android-sdk, with platforms/android-35, build-tools/35.0.0 and
#     ndk/28.2.13676358 (install them with cmdline-tools\latest\bin\android.exe)
#   - Rust's aarch64-linux-android target (and x86_64-linux-android for -Emulator), and
#     cargo-ndk (cargo install cargo-ndk)
#   - a JDK, for javac and keytool
#
# The .apk is signed with the key in the tools folder's kaye-android. It's made the first
# time, and it must never change or be lost: Android only installs an update signed with the
# same key as the copy already on the phone. Keep a backup of that folder. Never put it in
# the repository, which is public.

param(
    [switch]$Emulator,
    [switch]$Debug
)

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
$toolsRoot = if (Test-Path -LiteralPath 'E:\tools\android-sdk') { 'E:\tools' } else { Join-Path $env:USERPROFILE 'tools' }
$sdk = Join-Path $toolsRoot 'android-sdk'
$ndk = Join-Path $sdk 'ndk\28.2.13676358'
$platform = Join-Path $sdk 'platforms\android-35\android.jar'
$tools = Join-Path $sdk 'build-tools\35.0.0'
$keys = Join-Path $toolsRoot 'kaye-android'
foreach ($needed in $ndk, $platform, $tools) {
    if (-not (Test-Path -LiteralPath $needed)) { throw "$needed is missing. See the top of this script." }
}
$env:ANDROID_HOME = $sdk
$env:ANDROID_NDK_HOME = $ndk

$version = (Select-String -LiteralPath (Join-Path $project 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$parts = $version.Split('.') | ForEach-Object { [int]$_ }
# Android only installs a higher versionCode over an older one: 0.1.5 is 105, 1.2.3 is 10203.
$versionCode = $parts[0] * 10000 + $parts[1] * 100 + $parts[2]

function Invoke-Checked([string]$what, [scriptblock]$command) {
    & $command
    if ($LASTEXITCODE -ne 0) { throw "$what failed." }
}

# Where Cargo puts its output: .cargo\config.toml's target-dir, else target.
$targetDir = Join-Path $project 'target'
$config = Join-Path $project '.cargo\config.toml'
if (Test-Path -LiteralPath $config) {
    $line = Select-String -LiteralPath $config -Pattern '^target-dir\s*=\s*"(.+)"'
    if ($line) { $targetDir = $line.Matches[0].Groups[1].Value }
}

$work = Join-Path $targetDir 'apk'
if (Test-Path -LiteralPath $work) { Remove-Item -Recurse -Force -LiteralPath $work }
New-Item -ItemType Directory -Force $work | Out-Null
$staging = Join-Path $work 'staging'
$libs = Join-Path $staging 'lib'

# 1. Kaye itself, as libkaye_mobile.so, with the C++ library Luau needs beside it.
$abis = @('arm64-v8a')
if ($Emulator) { $abis += 'x86_64' }
$cargoArgs = @('ndk', '--platform', '26', '--link-libcxx-shared', '-o', $libs)
foreach ($abi in $abis) { $cargoArgs += @('-t', $abi) }
$cargoArgs += @('build', '-p', 'kaye-mobile')
if (-not $Debug) { $cargoArgs += '--release' }
Push-Location $project
try {
    Invoke-Checked 'Building Kaye for Android' { & cargo @cargoArgs }
} finally {
    Pop-Location
}
# Debugging symbols are most of the libraries' size, and a phone has no use for them.
if (-not $Debug) {
    $strip = Join-Path $ndk 'toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-strip.exe'
    foreach ($library in Get-ChildItem -Recurse -Filter *.so $libs) {
        Invoke-Checked "Stripping $($library.Name)" { & $strip --strip-unneeded $library.FullName }
    }
}

# 2. The Java side, as classes.dex.
$classes = Join-Path $work 'classes'
New-Item -ItemType Directory -Force $classes | Out-Null
$java = @(Get-ChildItem -Recurse -Filter *.java (Join-Path $project 'android\java') | ForEach-Object { $_.FullName })
Invoke-Checked 'Compiling the Java side' { & javac --release 11 -nowarn -classpath $platform -d $classes @java }
$classFiles = @(Get-ChildItem -Recurse -Filter *.class $classes | ForEach-Object { $_.FullName })
Invoke-Checked 'Converting it for Android' { & (Join-Path $tools 'd8.bat') --release --min-api 26 --lib $platform --output $staging @classFiles }

# 3. The manifest and icons, as the start of the .apk.
$manifest = Join-Path $work 'AndroidManifest.xml'
$text = (Get-Content -Raw -LiteralPath (Join-Path $project 'android\AndroidManifest.xml')).Replace('@VERSION_CODE@', "$versionCode").Replace('@VERSION_NAME@', $version)
[IO.File]::WriteAllText($manifest, $text, (New-Object Text.UTF8Encoding $false))
$compiled = Join-Path $work 'res.zip'
Invoke-Checked 'Packing the icons' { & (Join-Path $tools 'aapt2.exe') compile --dir (Join-Path $project 'android\res') -o $compiled }
$unsigned = Join-Path $work 'unsigned.apk'
Invoke-Checked 'Packing the manifest' { & (Join-Path $tools 'aapt2.exe') link -I $platform --manifest $manifest -o $unsigned $compiled }

# 4. Kaye and the Java side go in beside them. The libraries are compressed, which the
#    manifest's extractNativeLibs allows, so the download is a third the size.
Invoke-Checked 'Adding classes.dex' { & jar --update --file $unsigned --no-manifest -C $staging classes.dex }
Invoke-Checked 'Adding Kaye' { & jar --update --file $unsigned --no-manifest -C $staging lib }

# 5. Lined up and signed.
$aligned = Join-Path $work 'aligned.apk'
Invoke-Checked 'Aligning' { & (Join-Path $tools 'zipalign.exe') -f -P 16 4 $unsigned $aligned }
$keystore = Join-Path $keys 'kaye-release.jks'
$passFile = Join-Path $keys 'password.txt'
if (-not (Test-Path -LiteralPath $keystore)) {
    New-Item -ItemType Directory -Force $keys | Out-Null
    $password = -join ((48..57) + (65..90) + (97..122) | Get-Random -Count 32 | ForEach-Object { [char]$_ })
    [IO.File]::WriteAllText($passFile, $password)
    Invoke-Checked 'Making the signing key' {
        & keytool -genkeypair -keystore $keystore -storepass $password -keypass $password -alias kaye -keyalg RSA -keysize 4096 -validity 36500 -dname 'CN=Kaye, O=Toorium' | Out-Null
    }
    Write-Host "Made a new signing key at $keys. Back that folder up: without it, no update can install over this one."
}
$dist = Join-Path $project 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$suffix = if ($Debug) { '-debug' } elseif ($Emulator) { '-emulator' } else { '' }
$apk = Join-Path $dist "Kaye-$version$suffix.apk"
Invoke-Checked 'Signing' { & (Join-Path $tools 'apksigner.bat') sign --ks $keystore --ks-key-alias kaye --ks-pass "file:$passFile" --out $apk $aligned }
Remove-Item -Force -ErrorAction SilentlyContinue "$apk.idsig"
$size = [math]::Round((Get-Item -LiteralPath $apk).Length / 1MB, 1)
Write-Host "Built $apk ($size MB)"
