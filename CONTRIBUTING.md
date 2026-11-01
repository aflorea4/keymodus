# Contributing to Keymodus

Thanks for helping! A few notes to make it smooth.

## Getting started

```sh
cargo install tauri-cli --version '^2' --locked
cargo test --workspace
cd app && cargo tauri dev
```

On Linux, install `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libudev-dev`
first, plus the udev rule from `packaging/linux`.

The UI is plain HTML/CSS/JS with no build step. Serve `ui/` over HTTP to work on
it with the mock backend (`ui/mock.js`).

## Before opening a pull request

- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Keep platform-specific code inside `app/src/platform/<os>.rs`, behind the
  shared functions listed in `app/src/platform/mod.rs`.
- New HID++ features go in `crates/hidpp/src/features.rs` with a unit test using
  the fake transport. On a feature, call **only the documented getter functions**
  when probing: on many features function 1 or 2 is a setter, and calling it
  with empty parameters resets the device's settings.
- Configuration changes must stay backwards compatible: new fields need serde
  defaults; bump `SCHEMA_VERSION` and extend `Config::migrate` if needed.

## Adding a device

Run `cargo run -p keymodus --example probe` and attach its output to an issue
(remove serial numbers). For a dedicated drawing, add an entry to `ui/devices.js`
keyed by `046d:<pid>`. Drawings must be original: no Logitech artwork or logos.

## Privacy in issues and commits

Don't include serial numbers, usernames, home paths or log lines with personal
data. Configure Git to use your GitHub no-reply email if you prefer to keep yours private.
