# Security policy

Keymodus sends input on your behalf and reads input from your devices, so
security reports are taken seriously.

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private
[Report a vulnerability](https://github.com/aflorea4/keymodus/security/advisories/new)
form instead. Include the version, OS, and steps to reproduce. You'll get an
acknowledgement within a few days. Fixes ship as a new release, which installed
copies are offered automatically.

## Supported versions

Only the latest release gets security fixes.

## What Keymodus does and doesn't do

- **Network:** the only connection is the update check, a request to this
  repository's GitHub Releases. There is no telemetry and no account.
- **Updates:** each download is verified against the minisign public key in
  `app/tauri.conf.json` before it's installed. A file that fails the check is
  rejected.
- **Input:** Keymodus only sees presses of controls you've remapped (they are
  diverted by the device), and on *All keyboards* only keys you've assigned.
  Nothing is logged except connection and binding-guard events.
- **Actions you configure:** "Run a command" runs what you typed with your
  user's permissions, like a shell alias. Settings files you import can contain
  such commands, so only import files you trust.
