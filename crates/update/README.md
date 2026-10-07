# MyAIPDF updates

L4, no GUI dependencies. Explicit checks only, against `ddxmu/MyAIPDF` stable GitHub Releases.
Downloads are bounded and verified against the release asset's SHA-256 digest; arbitrary
repositories, download hosts and filenames are rejected. No AI keys or PDF data are sent.

`check_latest`, `download`, `verify_package` and `install` are shared by the desktop app and
headless update tools. Installation requires an explicit user action and a closed target app.
The macOS installer checks bundle identity/version and deep code-signature integrity, stages
beside the existing bundle, preserves the old bundle, and rolls back if activation fails.
It does not change the settings/keychain directory or bypass macOS security protections.
