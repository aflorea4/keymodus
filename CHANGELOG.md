# Changelog

## 0.2.0 — 2026-11-01

First public release.

### Added
- **Windows and Linux** support, alongside macOS. Installers for all three are
  built by GitHub Actions on every tagged release.
- **Update agent:** checks GitHub Releases at launch and every 6 hours, shows a
  banner, and installs a signature-verified update with one click
  (Settings → Updates; can be turned off).
- **Options+-style device page:** device illustration with hotspots, battery,
  Easy-Switch channel strip (switch the device to another computer), an
  application-profile bar, and Buttons / Point & scroll / Keyboard / Device tabs.
- **Action picker** with search and Popular / Navigation presets; shortcuts
  shown with the right symbols for each OS.
- **Smart Actions:** saved multi-step sequences (shortcuts, pauses, apps,
  links, text) that any button or key can run.
- **Keyboard settings:** backlight, disable keys (e.g. Caps Lock) in hardware,
  and top-row F-key remapping on keyboards such as the MX Mechanical Mini.
- Original illustrations for LIFT for Mac and MX Mechanical Mini for Mac.
- Export and import of the settings file; "Reset buttons" per device.

### Changed
- Settings schema v2 (adds Smart Actions, backlight, disabled keys, update
  preference). v1 files are migrated automatically and the old file is kept
  as a backup.
- macOS releases are signed with a stable identity, so Accessibility and Input
  Monitoring stay granted across updates.

## 0.1.0

Initial macOS-only version (not published).
