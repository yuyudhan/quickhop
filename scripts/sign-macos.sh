#!/usr/bin/env bash
# Developer ID signing and notarization for the macOS release, when the signing variables below
# are set; without them quickhop.app stays as scripts/bundle-macos.sh signed it (ad hoc), which
# install.sh does not mind (curl sets no quarantine flag, and it clears one anyway) but a browser
# download does: Gatekeeper then calls the app "damaged" (docs/troubleshooting.md).
#
# Usage: scripts/sign-macos.sh APP BIN, from scripts/release-local.sh (`just release`), which
# sets GITHUB_OUTPUT and RUNNER_TEMP to its own files as the old GitHub workflow's runner did
#   APP  quickhop.app from scripts/bundle-macos.sh
#   BIN  the bare universal binary that goes beside it in the archive
#
# Environment (all optional; partial sets are an error):
#   MACOS_CERTIFICATE           base64 of the "Developer ID Application" certificate and key (.p12)
#   MACOS_CERTIFICATE_PASSWORD  the .p12 password
#   MACOS_SIGN_IDENTITY         the identity, e.g. "Developer ID Application: Name (TEAMID)"
#   NOTARY_KEY                  base64 of an App Store Connect API key (AuthKey_<id>.p8)
#   NOTARY_KEY_ID, NOTARY_ISSUER  that key's id and issuer id
#
# Writes notarized=true|false to GITHUB_OUTPUT (scripts/homebrew.sh reads it as
# QUICKHOP_NOTARIZED). A notary key without a certificate is an error: only Developer ID signed
# code can be notarized.
set -euo pipefail

[ "$#" -eq 2 ] || {
    echo "usage: $0 APP BIN" >&2
    exit 2
}
app="$1"
bin="$2"
: "${GITHUB_OUTPUT:?sign-macos.sh needs GITHUB_OUTPUT (scripts/release-local.sh sets it)}"
: "${RUNNER_TEMP:?sign-macos.sh needs RUNNER_TEMP (scripts/release-local.sh sets it)}"
if [ ! -d "$app" ] || [ ! -f "$bin" ]; then
    echo "sign-macos: ${app} or ${bin} is missing" >&2
    exit 2
fi

if [ -z "${MACOS_CERTIFICATE:-}" ]; then
    if [ -n "${NOTARY_KEY:-}" ]; then
        echo "sign-macos: NOTARY_KEY is set but MACOS_CERTIFICATE is not" >&2
        exit 1
    fi
    echo "sign-macos: no MACOS_CERTIFICATE secret, quickhop.app stays ad-hoc signed and is not notarized" >&2
    echo "notarized=false" >>"$GITHUB_OUTPUT"
    exit 0
fi
for name in MACOS_CERTIFICATE_PASSWORD MACOS_SIGN_IDENTITY; do
    if [ -z "${!name:-}" ]; then
        echo "sign-macos: MACOS_CERTIFICATE is set but ${name} is not" >&2
        exit 1
    fi
done

# The certificate goes into a temporary keychain in RUNNER_TEMP, first in the search list;
# scripts/release-local.sh deletes it (`security delete-keychain`) after signing.
keychain="${RUNNER_TEMP}/quickhop-signing.keychain-db"
keychain_password="$(uuidgen)"
certificate="${RUNNER_TEMP}/quickhop-certificate.p12"
printf '%s' "$MACOS_CERTIFICATE" | base64 --decode >"$certificate"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$certificate" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
rm -f "$certificate"
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
existing=()
while IFS= read -r line; do
    line="${line#"${line%%[![:space:]]*}"}"
    existing+=("${line//\"/}")
done < <(security list-keychains -d user)
security list-keychains -d user -s "$keychain" "${existing[@]}"
if ! security find-identity -v -p codesigning "$keychain" | grep -qF "\"${MACOS_SIGN_IDENTITY}\""; then
    echo "sign-macos: the certificate holds no valid identity named ${MACOS_SIGN_IDENTITY}" >&2
    exit 1
fi

# Hardened runtime and a secure timestamp, which notarization requires; the identifier is the
# bundle's, as bundle-macos.sh signs it ad hoc, so the bare binary and the app are one program.
bundle_id="$(plutil -extract CFBundleIdentifier raw -o - "${app}/Contents/Info.plist")"
sign=(codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY"
    --identifier "$bundle_id")
"${sign[@]}" "$bin"
"${sign[@]}" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
echo "sign-macos: signed ${app} and ${bin} as ${MACOS_SIGN_IDENTITY}" >&2

if [ -z "${NOTARY_KEY:-}" ]; then
    echo "sign-macos: no NOTARY_KEY secret, the release is signed but not notarized" >&2
    echo "notarized=false" >>"$GITHUB_OUTPUT"
    exit 0
fi
for name in NOTARY_KEY_ID NOTARY_ISSUER; do
    if [ -z "${!name:-}" ]; then
        echo "sign-macos: NOTARY_KEY is set but ${name} is not" >&2
        exit 1
    fi
done
key="${RUNNER_TEMP}/AuthKey_${NOTARY_KEY_ID}.p8"
printf '%s' "$NOTARY_KEY" | base64 --decode >"$key"
chmod 600 "$key"
zip="${RUNNER_TEMP}/quickhop-notarize.zip"
ditto -c -k --keepParent "$app" "$zip"
result="$(xcrun notarytool submit "$zip" --key "$key" --key-id "$NOTARY_KEY_ID" \
    --issuer "$NOTARY_ISSUER" --wait --timeout 30m --output-format json)"
echo "$result" >&2
if ! grep -Eq '"status" *: *"Accepted"' <<<"$result"; then
    echo "sign-macos: notarization was not accepted; see: xcrun notarytool log <id>" >&2
    exit 1
fi
rm -f "$zip" "$key"
spctl --assess --type execute --verbose=2 "$app"
echo "sign-macos: ${app} is notarized and stapled" >&2
echo "notarized=true" >>"$GITHUB_OUTPUT"
