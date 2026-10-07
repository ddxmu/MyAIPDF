# MyAIPDF update helper

Packaged beside the GUI, copied to a private cache before launch. Waits up to three minutes
for the parent GUI to exit normally; never kills an app. A still-open GUI aborts installation.
`printcraft-update` verifies the package, bundle identity, version and code signature, checks
the target is closed, and preserves the old bundle for recovery. Success reopens MyAIPDF.
No root privileges, shell scripts, Gatekeeper overrides, PDF access or credential access.

Usage (normally launched by the GUI):
`myaipdf-updater --package /absolute/package.json --application /absolute/MyAIPDF.app --wait-pid PID`
