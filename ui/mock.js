// Mock backend so the UI can be previewed in a plain browser (no Tauri).
// Mirrors the shapes returned by app/src/main.rs. All ids and serials here are
// made up.
//
// URL flags: ?os=windows|linux  ?noperm  ?op (Options+ running)  ?empty
//            ?update (pending update)  ?art (use a local Options+ depot symlinked at ui/art)

const LOCAL_ART = "art";

export function createMockBackend() {
  const listeners = {};
  const now = Date.now();
  const params = new URLSearchParams(location.search);
  const useArt = location.protocol.startsWith("http") && params.has("art");
  const os = params.get("os") || "macos";
  const state = {
    devices: [
      {
        id: "046d:b031", name: "LIFT for Mac", kind: "mouse", connection: "Bluetooth", status: "online",
        battery: { percent: 82, charging: false, level: "good" },
        controls: [
          { cid: 82, name: "Middle button", gesture: true, mouse: true, fkey: null },
          { cid: 253, name: "Top button", gesture: true, mouse: true, fkey: null },
          { cid: 86, name: "Forward button", gesture: true, mouse: true, fkey: null },
          { cid: 83, name: "Back button", gesture: true, mouse: true, fkey: null },
        ],
        art: useArt ? { url: `${LOCAL_ART}/mx_vertical_mini/front_ext_2.webp`, width: 396, height: 396, hotspots: [
          { cid: 82, name: "Middle button", x: 59, y: 18, side: 1 }, { cid: 86, name: "Forward button", x: 21, y: 32, side: -1 },
          { cid: 83, name: "Back button", x: 28, y: 50, side: -1 }, { cid: 253, name: "Top button", x: 67, y: 37, side: 1 } ] } : null,
        features: [["0x0000", "Root"], ["0x0001", "Feature set"], ["0x0003", "Device information"], ["0x0005", "Device name & type"], ["0x1004", "Unified battery"], ["0x1814", "Change host"], ["0x1815", "Hosts info"], ["0x1B04", "Reprogrammable controls"], ["0x1D4B", "Wireless status"], ["0x2130", "Ratchet wheel"], ["0x2201", "Adjustable DPI"]],
        firmware: ["Firmware: RBM 00.00.B0000", "Bootloader: BOT 00.00.B0000"], serial: "0000XX000000",
        dpi: { current: 1000, supported: Array.from({ length: 40 }, (_, i) => 400 + i * 100) },
        wheel: null, smartshift: null, fn_inverted: null, host: [3, 0], backlight: null, disable_keys: null,
        guard: { diverted: 2, verified_at: now - 6000, restores: 3 },
      },
      {
        id: "046d:b36d", name: "MX Mechanical Mini for Mac", kind: "keyboard", connection: "Bluetooth", status: "online",
        battery: { percent: 64, charging: false, level: "good" },
        controls: [226, 227, 259, 264, 266, 284, 212, 321, 231].map((cid, i) => ({ cid, name: `F${i + 4} key`, gesture: false, mouse: false, fkey: i + 4 })),
        art: useArt ? { url: `${LOCAL_ART}/mx_mechanical_mini_mac/front.webp`, width: 1872, height: 728, hotspots: [226, 227, 259, 264, 266, 284, 212, 321, 231, 232, 233, 285].map((cid, i) => ({ cid, name: "", x: [25.9, 32, 38.15, 44.4, 50.5, 56.7, 63, 69, 75.2, 81.3, 87.35, 93.6][i] + 2.4, y: 11 + 8.65, side: -1 })) } : null,
        features: [["0x0000", "Root"], ["0x1B04", "Reprogrammable controls"], ["0x1814", "Change host"], ["0x1982", "Backlight"], ["0x1A20", "Ambient light sensor"], ["0x40A3", "Fn inversion"], ["0x4521", "Disable keys"], ["0x4531", "Multiplatform"]],
        firmware: ["Firmware: MPK 00.00.B0000"], serial: "0000YY000000",
        dpi: null, wheel: null, smartshift: null, fn_inverted: false, host: [3, 1],
        backlight: { enabled: true, mode: "basic", level: 0, levels: 8, auto_supported: false, manual_supported: false }, disable_keys: [1, 0],
        guard: { diverted: 1, verified_at: now - 11000, restores: 0 },
      },
    ],
    config: {
      version: 2,
      devices: {
        "046d:b031": { name: "LIFT for Mac", kind: "mouse", bindings: { 83: { type: "keystroke", key: "BracketLeft", mods: ["cmd"] }, 253: { type: "cycle_dpi" } }, app_bindings: { "com.apple.Safari": { 86: { type: "keystroke", key: "KeyT", mods: ["cmd", "shift"] } } }, pointer: { dpi_presets: [800, 1600, 2400] } },
        "046d:b36d": { name: "MX Mechanical Mini for Mac", kind: "keyboard", bindings: { 266: { type: "system", action: "screenshot_region" } }, app_bindings: {}, pointer: {} },
      },
      generic: { mouse_buttons: { 3: { type: "system", action: "navigate_back" } }, keys: {} },
      settings: { launch_at_login: true, auto_update_check: true, gesture_threshold: 60 },
      smart_actions: {
        sa1: { name: "Search selection", steps: [{ kind: "action", action: { type: "keystroke", key: "KeyC", mods: [os === "macos" ? "cmd" : "ctrl"] } }, { kind: "delay", ms: 150 }, { kind: "action", action: { type: "open_url", url: "https://duckduckgo.com" } }] },
      },
    },
    capabilities: { os, per_app: os !== "linux", any_device: os !== "linux", options_plus: os === "macos", unsupported_system: [], unsupported_media: os === "windows" ? ["brightness_up", "brightness_down", "keyboard_brightness_up", "keyboard_brightness_down"] : [] },
    config_dir: { macos: "~/Library/Application Support/Keymodus", windows: "%APPDATA%\\Keymodus", linux: "~/.config/keymodus" }[os],
    update: params.has("update") ? { version: "0.2.1", current: "0.2.0", notes: "Fixes", date: null } : null,
    permissions: { accessibility: !params.has("noperm"), input_monitoring: true },
    options_plus: { installed: true, running: params.has("op") },
    guard_log: [
      { at: now - 60000 * 3, device: "LIFT for Mac", message: "Device had dropped 2 binding(s) — restored (woke from idle)", tone: "restore" },
      { at: now - 60000 * 40, device: "MX Mechanical Mini for Mac", message: "1 binding(s) active (connected)", tone: "info" },
      { at: now - 60000 * 41, device: "LIFT for Mac", message: "Connected over Bluetooth/USB", tone: "info" },
    ],
    notices: [],
    launch_at_login: true,
    backups: ["config-20261014-101500-000000.json", "config-20261014-093012-000000.json"],
    version: "0.2.0",
  };
  if (params.has("empty")) state.devices = [];

  const clone = (x) => JSON.parse(JSON.stringify(x));
  const handlers = {
    get_state: () => clone(state),
    set_binding: ({ device, cid, app, action }) => {
      const d = (state.config.devices[device] ??= { bindings: {}, app_bindings: {}, pointer: {} });
      if (app) (d.app_bindings[app] ??= {})[cid] = action;
      else if (action.type === "default") delete d.bindings[cid];
      else d.bindings[cid] = action;
      return clone(state.config);
    },
    clear_app_binding: ({ device, cid, app }) => { delete state.config.devices[device].app_bindings[app]?.[cid]; return clone(state.config); },
    set_pointer: ({ device, pointer }) => { state.config.devices[device].pointer = pointer; return clone(state.config); },
    set_fn_inverted: ({ device, on }) => { state.config.devices[device].fn_inverted = on; return clone(state.config); },
    set_generic_binding: ({ kind, code, action }) => {
      const m = state.config.generic[kind === "mouse" ? "mouse_buttons" : "keys"];
      action.type === "default" ? delete m[code] : (m[code] = action);
      return clone(state.config);
    },
    list_apps: () => [["Safari", "com.apple.Safari"], ["Google Chrome", "com.google.Chrome"], ["Figma", "com.figma.Desktop"], ["Visual Studio Code", "com.microsoft.VSCode"], ["Spotify", "com.spotify.client"], ["Finder", "com.apple.finder"]].map(([name, bundle_id]) => ({ name, bundle_id, path: `/Applications/${name}.app` })),
    set_backlight: ({ device, backlight }) => { state.config.devices[device].backlight = backlight; return clone(state.config); },
    set_disabled_keys: ({ device, mask }) => { state.config.devices[device].disabled_keys = mask; return clone(state.config); },
    remove_app_profile: ({ device, app }) => { delete state.config.devices[device].app_bindings[app]; return clone(state.config); },
    reset_device: ({ device }) => { Object.assign(state.config.devices[device], { bindings: {}, app_bindings: {} }); return clone(state.config); },
    switch_host: () => null,
    save_smart_action: ({ id, action }) => { state.config.smart_actions[id && state.config.smart_actions[id] ? id : "sa" + Date.now()] = action; return clone(state.config); },
    delete_smart_action: ({ id }) => { delete state.config.smart_actions[id]; return clone(state.config); },
    check_update: () => null,
    export_config: () => "keymodus-settings.json",
    import_config: ({ json }) => { state.config = JSON.parse(json); return clone(state.config); },
    set_auto_update: ({ on }) => { state.config.settings.auto_update_check = on; return null; },
    import_options_plus: () => ({ report: { imported: 0, kept_native: 9, devices: [], apps: [], skipped: [] }, config: clone(state.config) }),
  };
  const backend = {
    invoke: async (cmd, args) => (handlers[cmd] ? handlers[cmd](args || {}) : null),
    listen: (ev, cb) => { (listeners[ev] ??= []).push(cb); },
    emit: (ev, p) => (listeners[ev] || []).forEach((cb) => cb(p)),
  };
  window.__mock = backend;
  return backend;
}
