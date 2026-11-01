# Reverse-engineering notes

These notes record what Keymodus's design is based on. They describe
interoperability facts (protocols, settings categories, file formats). No
Logitech code, firmware or artwork is copied or redistributed.

## Logi Options+ (macOS, 2.x)

### How it remaps, and why bindings get lost

- Buttons are remapped by **diverting** controls with HID++ feature `0x1B04`
  (Reprogrammable Controls v4, function 3 `setCidReporting`). The divert flag
  lives in device RAM. Sleep, Easy-Switch, a Bluetooth reconnect or a battery
  dip clears it, and the control goes back to its factory function.
- The agent re-diverts on reconnect, but that path has no retry and no
  read-back. When the request lands mid-reconnect, it logs
  `Divert/Undivert request failed` and gives up until restart.
- All settings are one JSON document stored in a single row of
  `~/Library/Application Support/LogiOptionsPlus/settings.db` (table `data`,
  column `file`). It is rewritten wholesale on every change, and the
  `snapshots` table stays empty.

Keymodus's binding guard and crash-safe store answer these two problems (see
`app/src/engine.rs` and `crates/core/src/store.rs`). The importer reads that
database read-only (`crates/core/src/import.rs`).

### Device catalog on disk

`/Library/Application Support/Logi/LogiOptionsPlus/`:

```text
devices.json                             modelId ("2b031" = transport 2/BLE + PID B031) -> depot name
depots/<build>/<depot>/manifest.json     modelId[_extN] -> device image + metadata file
depots/<build>/<depot>/<metadata>.json   hotspots: slotId "<prefix>_c<cid>", marker x/y in percent
```

When Options+ is installed, Keymodus reads artwork and hotspots from these
files at runtime (`crates/core/src/catalog.rs`) and does not copy them.
Otherwise it uses its own drawings (`ui/devices.js`).

### Settings taxonomy (from the current installer)

The public installer (`logioptionsplus_installer.zip`) is a small downloader.
The agent itself arrives later from an authenticated "depot" pipeline, which
we did not fetch. The installer binary does embed the agent's internal message
routes, and those show which settings Options+ exposes per device. Each route
takes the form `/<area>/<device id>/<setting>`. Settings relevant to mice and
keyboards:

| Area | Settings | Keymodus |
|---|---|---|
| `devices` | `easy_switch`, `change_channel`, `fn_inversion`, `special_keys_divert_state`, `special_mouse_divert_state`, `hidden_slots`, `persistent_data` | Easy-Switch strip, Fn keys, binding guard |
| `mouse` | `pointer_speed`, `precision_mode`, `angle_snapping`, `dpi_always_on` | DPI + presets (precision/angle snapping: not yet) |
| `scrollwheel` | `ratchet_change` | SmartShift ratchet/free spin |
| `backlight` | `backlight_level`, `backlight_effect`, `backlight_duration`, `power_save` | Backlight on/off, adaptive/level where supported |
| `disable_keys` | `capabilities`, `settings/configure` | Disable keys |
| `keyboard` | `keyboard_oslayout`, `lock_key_pressed` | not yet (HID++ `0x4531` / `0x4220`) |
| `software_events` | `try_macro`, `shortcut/register` | Smart Actions, "Try it" |
| `hosts_info` | `hosts_names`, `keyboardlayout` | channel numbers only |

The same binary also carries the Smart Actions feature flag and a separate
Smart Actions feedback widget. This matches the product's model, where Smart
Actions are global, named sequences that any button can trigger. Keymodus
follows the same model (`Config.smart_actions`, `Action::SmartAction`).

## HID++ facts for the two reference devices

Read with `cargo run -p keymodus --example probe` over Bluetooth.

### LIFT for Mac (046D:B031)

- Features include `0x1004` battery, `0x1B04` v5 reprogrammable controls,
  `0x1814` change host, `0x1815` hosts info, `0x2130` (ratchet wheel; no hi-res
  `0x2121` and no SmartShift) and `0x2201` adjustable DPI.
- Divertable controls (CID, flags):
  - `0x52` middle (`0x31`)
  - `0x53` back (`0x31`)
  - `0x56` forward (`0x31`)
  - `0xFD` top button (`0x31`)
  - `0xD7` is a virtual gesture control (`0xA0`), hidden in the UI.
  - All four physical controls report raw XY, so they can be gesture buttons.

### MX Mechanical Mini for Mac (046D:B36D)

- Features include `0x1982` backlight v2, `0x1A20` ambient light sensor,
  `0x40A3` Fn inversion, `0x4220` lock key state, `0x4521` disable keys,
  `0x4531` multi-platform and `0x4540` keyboard layout.
- **Top row:** `0x1B04` lists F-keys with the F-key flag (`0x02`) and a position:
  - positions 1–3 (CIDs `0xD1`–`0xD3`) are the Easy-Switch keys and are not divertable;
  - positions 4–12 are divertable.
- The firmware also lists ~20 divertable controls without a position. These
  are internal virtual actions, so Keymodus only shows keys with an F-key
  position.
- **Backlight `0x1982`:**
  - Function 0 returns `enabled, options, supported, effects(LE16), level,
    hands-out/hands-in/powered timeouts (LE16 each)`.
  - Function 2 returns the number of levels (8).
  - On this keyboard `supported = 0x05`: the auto (`0x08`) and manual/permanent
    (`0x20`) modes are **not** host-selectable. Brightness is managed by the
    keyboard, and the host can only turn the backlight on or off.
- **Disable keys `0x4521`:** function 0 returns capabilities `0x01` (Caps Lock
  only), function 1 the current mask, and function 2 sets it.

### A trap worth knowing

On `0x1982`, **function 1 is the setter**. Probing it "to see what it returns"
with empty parameters writes `enabled = 0, options = 0` and turns the
backlight off. Keymodus's probe only calls known getter functions. Its
backlight writer always preserves the low option bits and the timeouts it
read, and only selects modes the firmware advertises.
