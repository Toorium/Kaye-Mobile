#!/usr/bin/env bash
# Builds Kaye for Android as dist/Kaye-<version>.apk on Linux, the same way
# scripts/build-apk.ps1 does on Windows. .github/workflows/android.yml runs it on GitHub.
#
# It needs ANDROID_HOME with platforms/android-35, build-tools/35.0.0 and
# ndk/28.2.13676358, Rust's aarch64-linux-android target, cargo-ndk and a JDK.
#
# Signing: with KAYE_KEYSTORE (a .jks file) and KAYE_KEYSTORE_PASSWORD set, the .apk is signed
# with that key, which must be the same kaye-release.jks build-apk.ps1 uses, or Android won't
# install it over Kaye already on a phone. Without them it's signed with a throwaway key, and
# installs only after Kaye is uninstalled.

set -euo pipefail

project="$(cd "$(dirname "$0")/.." && pwd)"
sdk="${ANDROID_HOME:?ANDROID_HOME is not set}"
ndk="$sdk/ndk/28.2.13676358"
platform="$sdk/platforms/android-35/android.jar"
tools="$sdk/build-tools/35.0.0"
for needed in "$ndk" "$platform" "$tools"; do
    [ -e "$needed" ] || { echo "$needed is missing. See the top of this script." >&2; exit 1; }
done
export ANDROID_NDK_HOME="$ndk"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$project/Cargo.toml" | head -n1)"
IFS=. read -r major minor patch <<< "$version"
version_code=$((major * 10000 + minor * 100 + patch))

work="$project/target/apk"
rm -rf "$work"
staging="$work/staging"
libs="$staging/lib"
mkdir -p "$libs"

# 1. Kaye itself, as libkaye_mobile.so, with the C++ library Luau needs beside it.
(cd "$project" && cargo ndk --platform 26 --link-libcxx-shared -o "$libs" -t arm64-v8a build -p kaye-mobile --release)
strip="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
find "$libs" -name '*.so' -exec "$strip" --strip-unneeded {} \;

# 2. The Java side, as classes.dex.
classes="$work/classes"
mkdir -p "$classes"
find "$project/android/java" -name '*.java' -print0 | xargs -0 javac --release 11 -nowarn -classpath "$platform" -d "$classes"
find "$classes" -name '*.class' -print0 | xargs -0 "$tools/d8" --release --min-api 26 --lib "$platform" --output "$staging"

# 3. The manifest and icons, as the start of the .apk.
manifest="$work/AndroidManifest.xml"
sed -e "s/@VERSION_CODE@/$version_code/" -e "s/@VERSION_NAME@/$version/" "$project/android/AndroidManifest.xml" > "$manifest"
"$tools/aapt2" compile --dir "$project/android/res" -o "$work/res.zip"
unsigned="$work/unsigned.apk"
"$tools/aapt2" link -I "$platform" --manifest "$manifest" -o "$unsigned" "$work/res.zip"

# 4. Kaye and the Java side go in beside them.
jar --update --file "$unsigned" --no-manifest -C "$staging" classes.dex
jar --update --file "$unsigned" --no-manifest -C "$staging" lib

# 5. Lined up and signed.
aligned="$work/aligned.apk"
"$tools/zipalign" -f -P 16 4 "$unsigned" "$aligned"
keystore="${KAYE_KEYSTORE:-}"
password="${KAYE_KEYSTORE_PASSWORD:-}"
suffix=""
if [ -z "$keystore" ] || [ ! -s "$keystore" ]; then
    echo "No signing key given: signing with a throwaway one. Uninstall Kaye before installing this build." >&2
    keystore="$work/throwaway.jks"
    password="throwaway-$RANDOM$RANDOM"
    keytool -genkeypair -keystore "$keystore" -storepass "$password" -keypass "$password" -alias kaye \
        -keyalg RSA -keysize 2048 -validity 365 -dname 'CN=Kaye test build' > /dev/null
    suffix="-test"
fi
mkdir -p "$project/dist"
apk="$project/dist/Kaye-$version$suffix.apk"
"$tools/apksigner" sign --ks "$keystore" --ks-key-alias kaye --ks-pass "pass:$password" --out "$apk" "$aligned"
rm -f "$apk.idsig"
echo "Built $apk ($(du -h "$apk" | cut -f1))"
