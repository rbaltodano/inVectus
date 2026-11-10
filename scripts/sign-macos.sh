#!/bin/sh
# Sign, notarize and staple the macOS app, then pack the release archive.
#
#   scripts/sign-macos.sh [path/to/inVectus.app] [output.tar.gz]
#
# Needs a "Developer ID Application" identity in the keychain and notary credentials saved
# once with `xcrun notarytool store-credentials invectus-notary --apple-id … --team-id …`
# (the password is typed into that prompt, never stored in this repository).
# Override with SIGN_IDENTITY and NOTARY_PROFILE.
set -eu

APP=${1:-target/release/bundle/macos/inVectus.app}
OUT=${2:-target/release/inVectus-macos-arm64.tar.gz}
PROFILE=${NOTARY_PROFILE:-invectus-notary}
IDENTITY=${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}

[ -d "$APP" ] || { echo "no app bundle at $APP" >&2; exit 1; }
[ -n "$IDENTITY" ] || { echo "no Developer ID Application identity in the keychain" >&2; exit 1; }
echo "signing with: $IDENTITY"

# Inner executables first, then the bundle; hardened runtime and a secure timestamp are
# required for notarization. The app runs without extra entitlements (checked 2026-11-10:
# ONNX Runtime embedding and llama.cpp on Metal both work under the hardened runtime).
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP/Contents/MacOS/invectus"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ditto -c -k --keepParent "$APP" "$work/notarize.zip"
xcrun notarytool submit "$work/notarize.zip" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"

tar -czf "$OUT" -C "$(dirname "$APP")" "$(basename "$APP")"
shasum -a 256 "$OUT"
