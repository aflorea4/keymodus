// Keymodus UI. Vanilla JS, no build step. Talks to the Rust backend via
// Tauri's invoke/listen; falls back to a mock backend when opened in a browser.

import { createMockBackend } from "./mock.js";
import { DEVICE_ART } from "./devices.js";

const T = window.__TAURI__;
const backend = T
  ? { invoke: (cmd, args) => T.core.invoke(cmd, args), listen: (ev, cb) => T.event.listen(ev, (e) => cb(e.payload)) }
  : createMockBackend();

const $ = (sel, root = document) => root.querySelector(sel);
const pad = (n) => String(n).padStart(2, "0");
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const state = {
  snap: null,
  sel: null, // {type:"device", id} | {type:"generic", kind:"mouse"|"key"} | {type:"smart"}
  tab: "buttons",
  scope: null, // bundle id or null (all apps)
  editing: null, // cid / generic code
  cat: null,
  gestureSlot: "click",
  apps: null,
  appQuery: "",
  popover: false,
  sheet: null, // "activity" | "settings"
  discovered: new Set(), // generic mouse buttons seen this session
  lastDevicesKey: "",
  firstListRender: true,
  query: "", // action search
  confirmHost: null, // Easy-Switch channel awaiting confirmation
  smartId: null, // selected Smart Action id
  smartAdd: null, // category while adding a step
  updateProgress: null,
};

const prefs = {
  get(k, d) { try { return JSON.parse(localStorage.getItem("ld." + k)) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem("ld." + k, JSON.stringify(v)); } catch {} },
};

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

const OS = () => state.snap?.capabilities?.os ?? (navigator.platform.startsWith("Mac") ? "macos" : navigator.platform.startsWith("Win") ? "windows" : "linux");
const IS_MAC = () => OS() === "macos";
const MOD_SYMS = {
  macos: { cmd: "⌘", shift: "⇧", option: "⌥", ctrl: "⌃", fn: "fn" },
  windows: { cmd: "Win", shift: "Shift", option: "Alt", ctrl: "Ctrl", fn: "Fn" },
  linux: { cmd: "Super", shift: "Shift", option: "Alt", ctrl: "Ctrl", fn: "Fn" },
};
const modSym = (m) => (MOD_SYMS[OS()] ?? MOD_SYMS.linux)[m];
const MOD_ORDER = ["ctrl", "option", "shift", "cmd", "fn"];
/** The platform's main shortcut modifier: ⌘ on macOS, Ctrl elsewhere. */
const PRIMARY = () => (IS_MAC() ? "cmd" : "ctrl");
const KEY_SYM = {
  Space: "Space", Enter: "↩", Escape: "esc", Backspace: "⌫", Delete: "⌦", Tab: "⇥",
  ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓",
  PageUp: "⇞", PageDown: "⇟", Home: "↖", End: "↘", PrintScreen: "PrtSc", Insert: "Ins", CapsLock: "⇪",
  Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\",
  Semicolon: ";", Quote: "'", Backquote: "`", Comma: ",", Period: ".", Slash: "/",
};
const keyLabel = (code) => KEY_SYM[code] ?? code.replace(/^Key/, "").replace(/^Digit/, "");
const comboText = (mods, key) => {
  const sorted = [...(mods || [])].sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b)).map(modSym);
  return IS_MAC() ? sorted.join("") + keyLabel(key) : [...sorted, keyLabel(key)].join("+");
};

const MEDIA = [
  ["play_pause", "Play / pause"], ["next", "Next track"], ["previous", "Previous track"],
  ["volume_up", "Volume up"], ["volume_down", "Volume down"], ["mute", "Mute"],
  ["brightness_up", "Display brighter"], ["brightness_down", "Display dimmer"],
  ["keyboard_brightness_up", "Keyboard light up"], ["keyboard_brightness_down", "Keyboard light down"],
];
const mediaList = () => MEDIA.filter(([k]) => !(state.snap?.capabilities?.unsupported_media ?? []).includes(k));

/** System actions with per-OS names and the shortcut each one performs. */
function systemGroups() {
  const os = OS();
  const n = (mac, win, lin) => (os === "macos" ? mac : os === "windows" ? win : lin);
  const h = (mac, win, lin) => n(mac, win, lin) || "";
  return [
    ["Windows & desktops", [
      ["mission_control", n("Mission Control", "Task view", "Activities overview"), h("", "Win+Tab", "Super")],
      ["app_expose", n("App windows", "Task view", "Activities overview"), h("", "Win+Tab", "Super")],
      ["show_desktop", "Show desktop", h("", "Win+D", "Super+D")],
      ["launchpad", n("Launchpad / Apps", "Start menu", "App grid"), h("", "Win", "Super+A")],
      ["previous_space", n("Space to the left", "Desktop to the left", "Workspace above"), h("⌃←", "Ctrl+Win+←", "Super+PgUp")],
      ["next_space", n("Space to the right", "Desktop to the right", "Workspace below"), h("⌃→", "Ctrl+Win+→", "Super+PgDn")],
      ["switch_app", "Previous app", h("⌘⇥", "Alt+Tab", "Alt+Tab")],
    ]],
    ["Navigation", [["navigate_back", "Back", h("⌘[", "Alt+←", "Alt+←")], ["navigate_forward", "Forward", h("⌘]", "Alt+→", "Alt+→")]]],
    ["Tools", [
      ["spotlight", n("Spotlight", "Search", "Search"), h("⌘Space", "Win+S", "Super")],
      ["screenshot_region", "Screenshot of area", h("⌘⇧4", "Win+Shift+S", "Shift+PrtSc")],
      ["screenshot", "Screenshot", h("⌘⇧3", "PrtSc", "PrtSc")],
      ["emoji_picker", "Emoji & symbols", h("⌃⌘Space", "Win+.", "Super+.")],
      ["toggle_microphone", "Mute microphone", h("", "Win+Alt+K", "")],
      ["lock_screen", "Lock screen", h("⌃⌘Q", "Win+L", "Super+L")],
    ]],
  ].map(([g, items]) => [g, items.filter(([k]) => !(state.snap?.capabilities?.unsupported_system ?? []).includes(k) && (os === "macos" || k !== "app_expose"))]);
}
const MOUSE = [[0, "Left click"], [1, "Right click"], [2, "Middle click"], [3, "Back (button 4)"], [4, "Forward (button 5)"]];
const nameOf = (list, k) => (list.find((x) => x[0] === k) || [])[1];
const systemName = (k) => systemGroups().flatMap((g) => g[1]).find((x) => x[0] === k)?.[1] ?? k;

/** Ready-made shortcuts, like the Options+ "popular" and "navigation" lists. */
function presetGroups() {
  const P = PRIMARY();
  const k = (key, mods) => ({ type: "keystroke", key, mods });
  return [
    ["Popular", [
      ["Copy", k("KeyC", [P])], ["Paste", k("KeyV", [P])], ["Cut", k("KeyX", [P])],
      ["Undo", k("KeyZ", [P])], ["Redo", IS_MAC() ? k("KeyZ", ["shift", "cmd"]) : k("KeyY", ["ctrl"])],
      ["Select all", k("KeyA", [P])], ["Find", k("KeyF", [P])], ["Save", k("KeyS", [P])],
      ["Screenshot of area", { type: "system", action: "screenshot_region" }],
      ["Mute microphone", { type: "system", action: "toggle_microphone" }],
      ["Play / pause", { type: "media", key: "play_pause" }],
      ["Emoji & symbols", { type: "system", action: "emoji_picker" }],
    ]],
    ["Navigation", [
      ["Back", { type: "system", action: "navigate_back" }], ["Forward", { type: "system", action: "navigate_forward" }],
      ["Next tab", k("Tab", ["ctrl"])], ["Previous tab", k("Tab", ["ctrl", "shift"])],
      ["New tab", k("KeyT", [P])], ["Close tab", k("KeyW", [P])], ["Reopen closed tab", k("KeyT", [P, "shift"])],
      ["Zoom in", k("Equal", [P])], ["Zoom out", k("Minus", [P])], ["Actual size", k("Digit0", [P])],
      ["Page up", k("PageUp", [])], ["Page down", k("PageDown", [])], ["Top of page", k("Home", [])], ["End of page", k("End", [])],
      [systemName("previous_space"), { type: "system", action: "previous_space" }], [systemName("next_space"), { type: "system", action: "next_space" }],
      ["Previous app", { type: "system", action: "switch_app" }],
    ]],
  ];
}

/** Flat catalogue for search: [label, action, group]. */
function catalogue() {
  const out = [];
  for (const [g, items] of presetGroups()) for (const [n, a] of items) out.push([n, a, g]);
  for (const [k, n] of mediaList()) out.push([n, { type: "media", key: k }, "Media"]);
  for (const [g, items] of systemGroups()) for (const [k, n] of items) out.push([n, { type: "system", action: k }, "System"]);
  for (const [b, n] of MOUSE) out.push([n, { type: "mouse_button", button: b }, "Mouse"]);
  for (const [id, sa] of Object.entries(state.snap?.config?.smart_actions ?? {})) out.push([sa.name, { type: "smart_action", id }, "Smart Actions"]);
  const seen = new Set();
  return out.filter(([n, a]) => { const key = n + JSON.stringify(a); if (seen.has(key)) return false; seen.add(key); return true; });
}
const sameAction = (a, b) => !!a && !!b && JSON.stringify({ ...a, mods: a.mods && [...a.mods].sort() }) === JSON.stringify({ ...b, mods: b.mods && [...b.mods].sort() });

function keysHTML(k, big = false) {
  const mods = [...(k.mods || [])].sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b));
  return `<span class="keys${big ? " kbd-lg" : ""}">${mods.map((m) => `<kbd>${esc(modSym(m))}</kbd>`).join("")}<kbd>${esc(keyLabel(k.key))}</kbd></span>`;
}

function actionText(a) {
  if (!a) return "Default";
  switch (a.type) {
    case "default": return "Default";
    case "disabled": return "Off";
    case "keystroke": return comboText(a.mods, a.key);
    case "media": return nameOf(MEDIA, a.key) ?? a.key;
    case "mouse_button": return nameOf(MOUSE, a.button) ?? `Button ${a.button + 1}`;
    case "system": return systemName(a.action);
    case "launch_app": return "Open " + (a.path.split(/[\\/]/).pop() || "").replace(/\.(app|exe|desktop)$/i, "");
    case "open_url": try { return "Open " + new URL(a.url).host; } catch { return "Open link"; }
    case "shell": return "Run script";
    case "type_text": return `Type “${a.text.length > 14 ? a.text.slice(0, 14) + "…" : a.text}”`;
    case "cycle_dpi": return "Cycle pointer speed";
    case "gesture": return "Gesture";
    case "smart_action": return state.snap?.config?.smart_actions?.[a.id]?.name ?? "Missing Smart Action";
  }
  return a.type;
}
const actionHTML = (a) => (a?.type === "keystroke" ? keysHTML(a) : esc(actionText(a)));
const isBound = (a) => a && a.type !== "default";

const GLYPH = {
  mouse: `<svg viewBox="0 0 20 20"><rect x="5" y="2.5" width="10" height="15" rx="5"/><path d="M10 2.5v5"/></svg>`,
  keyboard: `<svg viewBox="0 0 20 20"><rect x="2" y="5" width="16" height="10"/><path d="M5 8h.01M8 8h.01M11 8h.01M14 8h1M6 12h8"/></svg>`,
  device: `<svg viewBox="0 0 20 20"><rect x="4" y="3" width="12" height="14"/></svg>`,
  anymouse: `<svg viewBox="0 0 20 20"><rect x="5" y="2.5" width="10" height="15" rx="5"/><path d="M10 2.5v5"/><path d="M2 6.5v7M18 6.5v7" opacity=".6"/></svg>`,
  smart: `<svg viewBox="0 0 20 20"><path d="M11 2.5L4.5 11h5l-1 6.5L15.5 9h-5l.5-6.5Z"/></svg>`,
  anykey: `<svg viewBox="0 0 20 20"><rect x="3.5" y="3.5" width="13" height="13"/><path d="M8 13l2-6 2 6M8.7 11h2.6"/></svg>`,
};
const CHEVRON = `<svg class="chev" viewBox="0 0 10 10"><path d="M2.5 4l2.5 2.5L7.5 4"/></svg>`;
const BACK = `<svg viewBox="0 0 20 20"><path d="M12 4.5L6.5 10l5.5 5.5"/></svg>`;
const CLOSE = `<svg viewBox="0 0 20 20"><path d="M5.5 5.5l9 9M14.5 5.5l-9 9"/></svg>`;

function batteryHTML(b) {
  if (!b) return "";
  const pct = b.percent ?? ({ full: 100, good: 70, low: 25, critical: 8 }[b.level] ?? null);
  if (pct == null) return "";
  const w = Math.max(1.5, (pct / 100) * 13);
  const color = pct <= 15 ? "var(--warn)" : "currentColor";
  return `<span class="battery" title="Battery"><svg viewBox="0 0 20 10"><rect x=".5" y=".5" width="16" height="9" rx="2" fill="none" stroke="currentColor" opacity=".55"/><rect x="2" y="2" width="${w}" height="6" rx="1" fill="${color}"/><rect x="17.5" y="3.2" width="1.6" height="3.6" rx=".8" fill="currentColor" opacity=".55"/></svg>${b.percent != null ? b.percent + "%" : esc(b.level)}${b.charging ? " · charging" : ""}</span>`;
}

function relTime(ms) {
  if (!ms) return "never";
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 5) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// ---------------------------------------------------------------------------
// Data helpers
// ---------------------------------------------------------------------------

const devices = () => state.snap?.devices ?? [];
const currentDevice = () => (state.sel?.type === "device" ? devices().find((d) => d.id === state.sel.id) : null);
const devCfg = (id) => state.snap?.config?.devices?.[id] ?? { bindings: {}, app_bindings: {}, pointer: {} };

function bindingFor(id, cid, scope = state.scope) {
  const dc = devCfg(id);
  const own = scope ? dc.app_bindings?.[scope]?.[cid] : undefined;
  if (own) return { action: own, override: true };
  return { action: dc.bindings?.[cid] ?? { type: "default" }, override: false, inherited: !!scope };
}

function appName(bundle) {
  const a = state.apps?.find((x) => x.bundle_id === bundle);
  if (a) return a.name;
  const tail = bundle.split(".").pop();
  return tail.charAt(0).toUpperCase() + tail.slice(1);
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function render() {
  if (!state.snap) return;
  renderSidebar();
  renderBanners();
  renderView();
  renderGuardPill();
}

function renderSidebar() {
  const list = $("#device-list");
  const ds = devices();
  list.innerHTML = ds.length
    ? ds.map((d, i) => {
        const sub = d.status === "offline" ? "Offline" : d.status === "asleep" ? "Asleep" : [d.connection, d.battery?.percent != null ? d.battery.percent + "%" : null].filter(Boolean).join(" / ");
        const cur = state.sel?.type === "device" && state.sel.id === d.id;
        return `<li><button class="device-row" data-dev="${esc(d.id)}" aria-current="${cur}">
          <span class="device-idx">${pad(i + 1)}</span><span class="device-glyph">${GLYPH[d.kind] ?? GLYPH.device}</span>
          <span style="min-width:0"><div class="device-name">${esc(d.name)}</div><div class="device-sub"><span class="status-dot" data-s="${d.status}"></span>${esc(sub)}</div></span>
        </button></li>`;
      }).join("")
    : `<li class="device-sub" style="padding:10px 18px">Scanning for devices…</li>`;

  const g = state.snap.config.generic;
  const nm = Object.keys(g.mouse_buttons || {}).length, nk = Object.keys(g.keys || {}).length;
  const row = (kind, name, n, idx) => {
    const cur = state.sel?.type === "generic" && state.sel.kind === kind;
    return `<li><button class="device-row" data-generic="${kind}" aria-current="${cur}">
      <span class="device-idx">${idx}</span><span class="device-glyph">${kind === "mouse" ? GLYPH.anymouse : GLYPH.anykey}</span>
      <span style="min-width:0"><div class="device-name">${name}</div><div class="device-sub">${n ? `${n} binding${n > 1 ? "s" : ""}` : "Any brand / any port"}</div></span>
    </button></li>`;
  };
  $("#generic-list").innerHTML = row("mouse", "All mice", nm, "M*") + row("key", "All keyboards", nk, "K*");

  const ns = Object.keys(state.snap.config.smart_actions || {}).length;
  const cur = state.sel?.type === "smart";
  $("#library-list").innerHTML = `<li><button class="device-row" data-smart-nav="1" aria-current="${cur}">
      <span class="device-idx">SA</span><span class="device-glyph">${GLYPH.smart}</span>
      <span style="min-width:0"><div class="device-name">Smart Actions</div><div class="device-sub">${ns ? `${ns} saved` : "Multi-step shortcuts"}</div></span>
    </button></li>`;
}

function renderGuardPill() {
  const ds = devices().filter((d) => d.status !== "offline");
  const active = ds.reduce((n, d) => n + (d.guard?.diverted || 0), 0);
  const recent = (state.snap.guard_log || []).find((e) => e.tone === "restore" && Date.now() - e.at < 10 * 60 * 1000);
  const dot = $("#guard-pill .guard-dot");
  dot.dataset.tone = recent ? "restore" : active ? "ok" : "idle";
  $("#guard-text").textContent = recent ? "Guard / restored" : active ? `Guard armed / ${pad(active)}` : "Guard idle";
}

function renderBanners() {
  const s = state.snap;
  const out = [];
  for (const n of s.notices || []) {
    out.push(banner(n.tone, n.tone === "warn" ? "Alert" : "Notice", n.title, n.body, `<button class="btn btn-ghost" data-act="dismiss" data-id="${esc(n.id)}">Dismiss</button>`));
  }
  if (s.update) {
    const p = state.updateProgress;
    const pct = p?.total ? Math.round((p.done / p.total) * 100) : null;
    out.push(banner("accent", "Update", `Keymodus ${s.update.version} is available`,
      p ? `Downloading${pct != null ? ` · ${pct}%` : "…"} The app restarts when it's done.` : `You have ${s.update.current}. The download is verified with the project's signing key before it's installed.`,
      p ? "" : `<button class="btn btn-ghost" data-act="release-notes">What's new</button><button class="btn btn-accent" data-act="install-update">Install &amp; restart</button>`));
  }
  if (!s.permissions.accessibility) {
    const linux = OS() === "linux";
    out.push(banner("accent", "Action", linux ? "Allow Keymodus to send input" : "Allow Keymodus to perform your shortcuts",
      linux
        ? "Keymodus sends keystrokes through /dev/uinput, which your user can't open yet. Install the udev rule that ships with the package (see the README), then log out and back in."
        : "macOS needs Accessibility access before buttons can send keystrokes, media keys or clicks. Keymodus never records what you type.",
      linux ? "" : `<button class="btn btn-primary" data-act="perm" data-kind="accessibility">Open Settings</button>`));
  }
  if (s.options_plus.running) {
    out.push(banner("warn", "Conflict", "Logi Options+ is also running",
      "Two apps remapping the same buttons undo each other — a common reason bindings seem to vanish. Pause Options+ while you use Keymodus.",
      `<button class="btn btn-primary" data-act="pause-op">Pause Options+</button>`));
  }
  const total = Object.values(s.config.devices).reduce((n, d) => n + Object.keys(d.bindings || {}).length + Object.keys(d.app_bindings || {}).length, 0);
  if (s.capabilities?.options_plus !== false && s.options_plus.installed && total === 0 && !prefs.get("importDismissed", false)) {
    out.push(banner("", "Import", "Bring over your Options+ setup",
      "Copy button assignments and app-specific profiles from Logi Options+. Nothing in Options+ is changed.",
      `<button class="btn btn-ghost" data-act="import-later">Not now</button><button class="btn btn-accent" data-act="import">Import</button>`));
  }
  if (IS_MAC() && !devices().some((d) => d.status !== "offline") && !s.permissions.input_monitoring) {
    out.push(banner("", "Hint", "Devices not showing up?",
      "On some Macs, reading from Bluetooth devices also needs Input Monitoring.",
      `<button class="btn" data-act="perm" data-kind="input">Allow Input Monitoring</button>`));
  }
  $("#banners").innerHTML = out.join("");
}

const banner = (tone, tag, title, body, actions) =>
  `<div class="banner" data-tone="${tone}"><span class="banner-tag">${tag}</span><div class="banner-text"><div class="banner-title">${esc(title)}</div><div class="banner-body">${esc(body)}</div></div><div class="banner-actions">${actions}</div></div>`;

function renderView() {
  const view = $("#view");
  if (state.sel?.type === "generic") return renderGeneric(view);
  if (state.sel?.type === "smart") return renderSmart(view);
  const d = currentDevice();
  if (!d) return renderEmpty(view);

  const isKb = d.kind === "keyboard";
  const meta = [
    d.status === "online" ? (d.connection || "Connected") : d.status === "asleep" ? "Asleep / wake it to change settings" : "Offline",
    batteryHTML(d.battery),
  ].filter(Boolean);

  const tabs = [["buttons", isKb ? "Keys" : "Buttons"]];
  if (d.dpi || d.wheel || d.smartshift) tabs.push(["pointer", "Point & scroll"]);
  if (isKb || d.fn_inverted != null || d.backlight || d.disable_keys) tabs.push(["keyboard", "Keyboard"]);
  tabs.push(["device", "Device"]);
  if (!tabs.some((t) => t[0] === state.tab)) state.tab = "buttons";

  view.innerHTML = `
    <header class="head">
      <div class="head-text">
        <div class="eyebrow">${meta.map((m, i) => (i ? `<span class="sep"></span>` : "") + `<span>${m}</span>`).join("")}</div>
        <h1 class="title" data-long="${d.name.length > 18}">${esc(d.name)}</h1>
      </div>
      ${hostStripHTML(d)}
    </header>
    <div class="tabs" role="tablist">
      ${tabs.map(([k, n]) => `<button class="tab" role="tab" data-tab="${k}" aria-selected="${state.tab === k}">${n}</button>`).join("")}
      <span class="tab-ink"></span>
    </div>
    ${state.tab === "buttons" && d.controls.length ? profileBarHTML(d) : ""}
    ${state.tab === "buttons" ? workbenchHTML(d) : state.tab === "pointer" ? pointerHTML(d) : state.tab === "keyboard" ? keyboardHTML(d) : aboutHTML(d)}`;
  placeInk(false);
  if (state.tab === "buttons") wireRecorder();
  if (state.popover) $("#app-search")?.focus({ preventScroll: true });
}

/** Easy-Switch channels, like the 1-2-3 keys on the device. */
function hostStripHTML(d) {
  if (!d.host || d.host[0] < 2) return "";
  const [n, cur] = d.host;
  const online = d.status === "online";
  const cells = Array.from({ length: n }, (_, i) =>
    `<button class="host" data-host="${i}" aria-pressed="${i === cur}" ${online && i !== cur ? "" : "disabled"} title="${i === cur ? "This computer" : `Switch to channel ${i + 1}`}">${i + 1}</button>`).join("");
  const confirm = state.confirmHost != null
    ? `<div class="host-confirm" role="alertdialog"><p><b>Switch to channel ${state.confirmHost + 1}?</b> ${esc(d.name)} will disconnect from this computer and connect to the device paired on that channel.</p>
        <div class="banner-actions"><button class="btn btn-ghost" data-act="host-cancel">Cancel</button><button class="btn btn-accent" data-act="host-go">Switch</button></div></div>`
    : "";
  return `<div class="host-wrap"><div class="host-label">Easy-Switch</div><div class="host-strip">${cells}</div>${confirm}</div>`;
}

/** "All applications" plus one chip per app profile, like Options+. */
function profileBarHTML(d) {
  if (state.snap.capabilities?.per_app === false) return "";
  const dc = devCfg(d.id);
  const profiles = Object.keys(dc.app_bindings || {});
  // A just-picked app shows up right away, before its first assignment.
  if (state.scope && !profiles.includes(state.scope)) profiles.push(state.scope);
  const chip = (b, label) => `<span class="pchip" aria-current="${state.scope === b || (!b && !state.scope)}">
      <button class="pchip-main" data-scope="${esc(b ?? "")}">${b ? `<span class="monogram">${esc(label[0])}</span>` : `<span class="monogram">∗</span>`}${esc(label)}</button>
      ${b ? `<button class="pchip-x" data-remove-profile="${esc(b)}" aria-label="Remove ${esc(label)} profile">${CLOSE}</button>` : ""}</span>`;
  let pop = "";
  if (state.popover) {
    const q = state.appQuery.toLowerCase();
    const apps = (state.apps || []).filter((a) => !profiles.includes(a.bundle_id) && (!q || a.name.toLowerCase().includes(q) || a.bundle_id.toLowerCase().includes(q))).slice(0, 40);
    const custom = q && !apps.some((a) => a.bundle_id.toLowerCase() === q) && /^[\w.\-]+$/.test(q);
    pop = `<div class="popover" role="menu" style="left:0;right:auto">
      <div class="pop-label">Settings for one application</div>
      <input class="pop-search" id="app-search" placeholder="${OS() === "windows" ? "Search apps or type chrome.exe" : "Search apps"}" value="${esc(state.appQuery)}" autocomplete="off" spellcheck="false">
      ${state.apps ? apps.map((a) => `<button class="pop-item" data-scope="${esc(a.bundle_id)}"><span class="monogram">${esc(a.name[0])}</span>${esc(a.name)}</button>`).join("") : `<div class="pop-label">Loading apps…</div>`}
      ${custom ? `<button class="pop-item" data-scope="${esc(q)}"><span class="monogram">+</span>Use “${esc(q)}”</button>` : ""}
    </div>`;
  }
  return `<div class="profile-bar">
      <span class="profile-label">Profile</span>
      ${chip(null, "All applications")}
      ${profiles.map((b) => chip(b, appName(b))).join("")}
      <span class="scope"><button class="pchip-add" data-act="scope" aria-haspopup="menu" aria-expanded="${state.popover}">+ Add application</button>${pop}</span>
    </div>`;
}

function placeInk(animate = true) {
  const ink = $(".tab-ink"), tab = $(`.tab[aria-selected="true"]`);
  if (!ink || !tab) return;
  if (!animate) ink.style.transition = "none";
  ink.style.transform = `translateX(${tab.offsetLeft + 12}px) scaleX(${(tab.offsetWidth - 24) / 10})`;
  if (!animate) requestAnimationFrame(() => (ink.style.transition = ""));
}

// ---- Buttons ---------------------------------------------------------------

function workbenchHTML(d) {
  if (!d.controls.length) {
    return `<div class="empty"><div class="empty-inner"><h2>No remappable controls</h2><p>${d.status === "online" ? "This device doesn't expose buttons that can be reassigned." : "Connect the device to see its buttons."}</p></div></div>`;
  }
  return `<div class="workbench">
    <section class="stage" aria-label="Device">${hudHTML(d)}${stageHTML(d)}</section>
    <section class="panel">${state.editing != null ? editorHTML(editTarget(d)) : controlListHTML(d)}</section>
  </div>`;
}

function hudHTML(d) {
  const bound = d.controls.filter((c) => isBound(bindingFor(d.id, c.cid).action)).length;
  const sig = d.status === "online" ? "<em>Live</em>" : d.status === "asleep" ? "Asleep" : "Offline";
  return `<div class="hud" aria-hidden="true"><i></i><i></i><i></i><i></i>
    <b class="hud-tl">ID ${esc(d.id.toUpperCase())}</b>
    <b class="hud-tr">Sig ${sig}</b>
    <b class="hud-bl">${d.status === "online" ? "Press a button to trace it" : "Wake device to trace"}</b>
    <b class="hud-br">Ctl ${pad(d.controls.length)} / Bound <em>${pad(bound)}</em></b></div>`;
}

function stageHTML(d) {
  const spots = [];
  let art = "";
  let ar = 1, max = 460;
  const own = DEVICE_ART?.[d.id];
  if (own && !d.art) {
    ar = own.ar; max = own.max;
    art = own.svg;
    for (const c of d.controls) {
      const p = c.fkey ? own.spots[`f${c.fkey}`] : own.spots[c.cid];
      if (p) spots.push({ cid: c.cid, x: p[0], y: p[1], side: p[2] ?? 1 });
    }
    // Controls the drawing doesn't place still get a pin along the edge.
    let free = 0;
    for (const c of d.controls) if (!spots.some((x) => x.cid === c.cid)) spots.push({ cid: c.cid, x: 94, y: 12 + 9 * free++, side: 1 });
  } else if (d.art) {
    ar = d.art.width / d.art.height;
    max = ar > 1.6 ? 820 : 380;
    art = `<img src="${esc(d.art.url)}" alt="${esc(d.name)}" draggable="false">`;
    for (const h of d.art.hotspots) {
      const c = d.controls.find((x) => x.cid === h.cid);
      if (c) spots.push({ cid: h.cid, x: h.x, y: h.y, side: h.side || 1 });
    }
  } else if (d.kind === "keyboard") {
    ar = 2.6; max = 760;
    art = schematicKeyboard();
    d.controls.forEach((c, i) => spots.push({ cid: c.cid, x: 8 + (84 * i) / Math.max(1, d.controls.length - 1), y: 18, side: 1 }));
  } else {
    ar = 0.72; max = 300;
    art = schematicMouse();
    const fixed = { 0x52: [50, 22, 1], 0xfd: [50, 42, 1], 0xc4: [50, 36, 1], 0x56: [17, 46, -1], 0x53: [17, 58, -1], 0xc3: [16, 70, -1], 0x5b: [36, 18, -1], 0x5d: [64, 18, 1] };
    let free = 0;
    for (const c of d.controls) {
      const f = fixed[c.cid] ?? [85, 30 + 10 * free++, 1];
      spots.push({ cid: c.cid, x: f[0], y: f[1], side: f[2] });
    }
  }
  const pin = spots.length > 6 || (own && d.kind === "keyboard");
  const items = spots.map((s) => {
    const i = d.controls.findIndex((c) => c.cid === s.cid);
    const b = bindingFor(d.id, s.cid);
    const bound = isBound(b.action);
    const c = d.controls[i];
    return `<div class="spot" style="--x:${s.x};--y:${s.y}" data-cid="${s.cid}" data-bound="${bound}" data-mode="${pin ? "pin" : "label"}" aria-current="${state.editing === s.cid}">
      <span class="spot-ring"></span>
      <button class="spot-btn" data-edit="${s.cid}" aria-label="${esc(c.name)}: ${esc(actionText(b.action))}"><span class="spot-dot"></span>${pin ? `<span class="spot-num">${c.fkey ?? i + 1}</span>` : ""}</button>
      ${pin ? "" : `<span class="spot-label" data-side="${s.side < 0 ? -1 : 1}"><span class="leader"></span><span class="tag">${bound ? actionHTML(b.action) : esc(c.name)}</span></span>`}
    </div>`;
  });
  return `<div class="art" style="--ar:${ar};--art-max:${max}px">${art}${items.join("")}</div>`;
}

function controlListHTML(d) {
  const enter = state.firstListRender ? " list-enter" : "";
  state.firstListRender = false;
  const scopeNote = state.scope ? `<span>${esc(appName(state.scope))}</span>` : "";
  return `<div class="panel-scroll">
    <div class="panel-title"><span>${d.kind === "keyboard" ? "Keys" : "Buttons"}</span>${scopeNote}</div>
    <ul class="control-list${enter}">
      ${d.controls.map((c, i) => {
        const b = bindingFor(d.id, c.cid);
        const bound = isBound(b.action);
        return `<li style="--i:${i}"><button class="control" data-edit="${c.cid}" data-cid="${c.cid}" data-bound="${bound}">
          <span class="control-num">${c.fkey ? "F" + c.fkey : i + 1}</span>
          <span><div class="control-name">${esc(c.name)}</div>
          <div class="control-action">${bound ? actionHTML(b.action) : "Default"}${b.override ? ` <span class="app-flag">· this app</span>` : ""}</div></span>
        </button></li>`;
      }).join("")}
    </ul>
  </div>`;
}

// ---- Editor (shared by devices and generic) -------------------------------------

function editTarget(d) {
  const c = d.controls.find((x) => x.cid === state.editing);
  if (!c) { state.editing = null; return null; }
  const b = bindingFor(d.id, c.cid);
  return {
    kind: "device", device: d.id, cid: c.cid, name: c.name, gesture: c.gesture,
    hasDpi: !!d.dpi, action: b.action, override: b.override, inherited: b.inherited,
  };
}

function categoryOf(a) {
  if (a && presetGroups().some(([, items]) => items.some(([, x]) => sameAction(x, a)))) {
    return presetGroups().find(([, items]) => items.some(([, x]) => sameAction(x, a)))[0].toLowerCase();
  }
  switch (a?.type) {
    case "smart_action": return "smart";
    case "keystroke": return "keys";
    case "media": return "media";
    case "mouse_button": case "cycle_dpi": return "mouse";
    case "system": return "system";
    case "launch_app": case "open_url": case "shell": return "open";
    case "type_text": return "text";
    case "gesture": return "gesture";
    case "disabled": return "off";
    default: return "keys";
  }
}

function editorHTML(t) {
  if (!t) return "";
  const cats = [["popular", "Popular"], ["keys", "Keystroke"], ["navigation", "Navigation"], ["media", "Media"], ["mouse", "Mouse"], ["system", "System"], ["open", "Open"], ["text", "Text"], ["smart", "Smart Action"]];
  if (t.gesture) cats.push(["gesture", "Gesture"]);
  cats.push(["off", "Off"]);
  if (!state.cat) state.cat = categoryOf(t.action);
  const subtitle = t.kind === "device"
    ? (state.scope ? (t.override ? `Only in ${esc(appName(state.scope))}` : `Inherits All apps · change to override in ${esc(appName(state.scope))}`) : "In all apps")
    : "On every device";
  const action = state.cat === "gesture" ? (t.action.type === "gesture" ? t.action : null) : t.action;
  return `<div class="editor" data-enter>
    <div class="editor-head">
      <button class="icon-btn back-btn" data-act="close-editor" aria-label="Back to list">${BACK}</button>
      <div style="flex:1;min-width:0"><h2>${esc(t.name)}</h2><p>${subtitle}</p></div>
    </div>
    <div class="editor-body">
      <input class="search" id="action-search" type="search" placeholder="Search actions" value="${esc(state.query)}" autocomplete="off" spellcheck="false" aria-label="Search actions">
      ${state.query ? searchHTML(t.action) : `<div class="cats" role="toolbar" aria-label="Action type">${cats.map(([k, n]) => `<button class="cat" data-cat="${k}" aria-pressed="${state.cat === k}">${n}</button>`).join("")}</div>
      ${state.cat === "gesture" ? gestureHTML(t, action) : pickerHTML(state.cat, t.action, t)}`}
    </div>
    <div class="editor-foot">
      ${t.override ? `<button class="btn btn-ghost" data-act="clear-override">Use All apps setting</button>` : `<button class="btn btn-ghost" data-act="set-default" ${isBound(t.action) ? "" : "disabled"}>Reset to default</button>`}
      <span class="spacer"></span>
      <button class="btn" data-act="try" ${isBound(t.action) ? "" : "disabled"}>Try it</button>
    </div>
  </div>`;
}

function searchHTML(cur, slot = "") {
  const q = state.query.toLowerCase().trim();
  const hits = catalogue().filter(([n, a, g]) => n.toLowerCase().includes(q) || g.toLowerCase().includes(q) || actionText(a).toLowerCase().includes(q)).slice(0, 30);
  const pre = slot ? `data-slot="${slot}" ` : "";
  return hits.length
    ? `<div class="options" role="radiogroup">${hits.map(([n, a, g]) => option(`${pre}data-pick='${esc(JSON.stringify(a))}'`, esc(n), sameAction(cur, a), a.type === "keystroke" ? actionText(a) : g)).join("")}</div>`
    : `<p class="help">No action matches “${esc(state.query)}”. Try Keystroke to record any shortcut.</p>`;
}

function option(attrs, label, checked, hint = "") {
  return `<button class="option" ${attrs} role="radio" aria-checked="${checked}"><span class="radio"></span><span>${label}</span>${hint ? `<span class="hint">${esc(hint)}</span>` : ""}</button>`;
}

/** Picker for one category. `cur` = currently assigned action. */
function pickerHTML(cat, cur, t, slot = "") {
  const pre = slot ? `data-slot="${slot}" ` : "";
  switch (cat) {
    case "keys":
      return `<div class="recorder" tabindex="0" ${pre}data-recorder>
          ${cur?.type === "keystroke" ? keysHTML(cur, true) : `<span class="keys kbd-lg"><kbd>${esc(modSym(PRIMARY()))}</kbd><kbd>…</kbd></span>`}
          <span class="prompt">Click here, then press a shortcut</span>
        </div>
        <p class="help" style="margin-top:10px">${t?.kind === "step" ? "Pressed and released once when the step runs." : `Held as long as you hold the button, so ${esc(modSym("shift"))} or ${esc(modSym(PRIMARY()))} work as modifiers too.`}</p>`;
    case "popular":
    case "navigation": {
      const group = presetGroups().find(([g]) => g.toLowerCase() === cat)?.[1] ?? [];
      return `<div class="options" role="radiogroup">${group.map(([n, a]) => option(`${pre}data-pick='${esc(JSON.stringify(a))}'`, esc(n), sameAction(cur, a), a.type === "keystroke" ? actionText(a) : "")).join("")}</div>`;
    }
    case "smart": {
      const all = Object.entries(state.snap.config.smart_actions || {});
      return `<div class="options" role="radiogroup">
          ${all.map(([id, sa]) => option(`${pre}data-pick='${esc(JSON.stringify({ type: "smart_action", id }))}'`, esc(sa.name), cur?.type === "smart_action" && cur.id === id, `${sa.steps.length} step${sa.steps.length === 1 ? "" : "s"}`)).join("")}
        </div>
        ${all.length ? "" : `<p class="help">A Smart Action runs several steps in a row: shortcuts, apps, links, text, pauses.</p>`}
        <button class="btn" data-act="new-smart" style="margin-top:12px;width:100%">Create a Smart Action</button>`;
    }
    case "media":
      return `<div class="options" role="radiogroup">${mediaList().map(([k, n]) => option(`${pre}data-pick='${JSON.stringify({ type: "media", key: k })}'`, n, cur?.type === "media" && cur.key === k)).join("")}</div>`;
    case "mouse":
      return `<div class="options" role="radiogroup">
        ${MOUSE.map(([b, n]) => option(`${pre}data-pick='${JSON.stringify({ type: "mouse_button", button: b })}'`, n, cur?.type === "mouse_button" && cur.button === b)).join("")}
        ${t?.hasDpi ? `<div class="group-label">Pointer</div>` + option(`${pre}data-pick='${JSON.stringify({ type: "cycle_dpi" })}'`, "Cycle pointer speed", cur?.type === "cycle_dpi", "presets in Pointer") : ""}
      </div>`;
    case "system":
      return `<div class="options" role="radiogroup">${systemGroups().map(([g, items]) => `<div class="group-label">${g}</div>` + items.map(([k, n, h]) => option(`${pre}data-pick='${JSON.stringify({ type: "system", action: k })}'`, n, cur?.type === "system" && cur.action === k, h)).join("")).join("")}</div>`;
    case "open": {
      const app = cur?.type === "launch_app" ? cur.path : "", url = cur?.type === "open_url" ? cur.url : "", sh = cur?.type === "shell" ? cur.command : "";
      return `<div class="field"><label for="f-app">Application</label>
          <input class="input" id="f-app" ${pre}data-field="launch_app" list="app-list" placeholder="${{ macos: "/Applications/Safari.app", windows: "C:\\Program Files\\App\\app.exe", linux: "firefox" }[OS()]}" value="${esc(app)}" spellcheck="false">
          <datalist id="app-list">${(state.apps || []).map((a) => `<option value="${esc(a.path)}">${esc(a.name)}</option>`).join("")}</datalist></div>
        <div class="field"><label for="f-url">Website or link</label>
          <input class="input" id="f-url" ${pre}data-field="open_url" placeholder="https://" value="${esc(url)}" spellcheck="false"></div>
        <div class="field"><label for="f-sh">Shell command</label>
          <textarea class="textarea" id="f-sh" ${pre}data-field="shell" placeholder="say hello" spellcheck="false">${esc(sh)}</textarea>
          <p class="help">Runs with ${OS() === "windows" ? "cmd.exe" : "/bin/sh"}, without a terminal. Press ${IS_MAC() ? "⌘↩" : "Ctrl+Enter"} to save.</p></div>`;
    }
    case "text":
      return `<div class="field"><label for="f-text">Text to type</label>
        <textarea class="textarea" id="f-text" ${pre}data-field="type_text" style="font-family:var(--font)" placeholder="Kind regards,">${esc(cur?.type === "type_text" ? cur.text : "")}</textarea>
        <p class="help">Typed wherever the cursor is. Press ${IS_MAC() ? "⌘↩" : "Ctrl+Enter"} to save.</p></div>`;
    case "off":
      return `<div class="options" role="radiogroup">
        ${option(`${pre}data-pick='${JSON.stringify({ type: "disabled" })}'`, "Do nothing", cur?.type === "disabled")}
        ${option(`${pre}data-pick='${JSON.stringify({ type: "default" })}'`, "Original function", !isBound(cur))}
      </div>`;
  }
  return "";
}

const EMPTY_GESTURE = () => ({ type: "gesture", click: { type: "default" }, up: { type: "system", action: "mission_control" }, down: { type: "system", action: "app_expose" }, left: { type: "system", action: "previous_space" }, right: { type: "system", action: "next_space" } });

function gestureHTML(t, g) {
  g = g ?? EMPTY_GESTURE();
  const cell = (k) => `<button class="g-cell" style="grid-area:${k}" data-gslot="${k}" aria-pressed="${state.gestureSlot === k}"><span class="dir">${k === "click" ? "Press" : k}</span><span class="what">${actionHTML(g[k])}</span></button>`;
  const slotAction = g[state.gestureSlot];
  const sub = ["keys", "media", "system", "mouse"].includes(state.gestureCat) ? state.gestureCat : categoryOf(slotAction) === "keys" || !["media", "system", "mouse"].includes(categoryOf(slotAction)) ? "keys" : categoryOf(slotAction);
  state.gestureCat = sub;
  return `<p class="help" style="margin:-4px 0 12px">Hold the button and move the mouse. A quick press does the centre action.</p>
    <div class="gesture-grid">${["up", "left", "click", "right", "down"].map(cell).join("")}</div>
    ${t.action.type !== "gesture" ? `<button class="btn btn-accent" data-act="make-gesture" style="width:100%;margin-bottom:14px">Use gestures on this button</button>` : ""}
    <div class="cats">${[["keys", "Shortcut"], ["media", "Media"], ["system", "System"], ["mouse", "Mouse"]].map(([k, n]) => `<button class="cat" data-gcat="${k}" aria-pressed="${sub === k}">${n}</button>`).join("")}</div>
    ${pickerHTML(sub, slotAction, t, state.gestureSlot)}`;
}

// ---- Pointer / keyboard / about --------------------------------------------

function pointerHTML(d) {
  const p = devCfg(d.id).pointer || {};
  const parts = [];
  if (d.dpi && d.dpi.supported.length) {
    const sup = d.dpi.supported;
    const cur = p.dpi ?? d.dpi.current;
    const idx = Math.max(0, sup.findIndex((v) => v >= cur));
    const pct = sup.length > 1 ? (idx / (sup.length - 1)) * 100 : 100;
    const suggest = [400, 800, 1000, 1200, 1600, 2000, 2400, 3200, 4000, 6400, 8000].filter((v) => v >= sup[0] && v <= sup[sup.length - 1]);
    const presets = p.dpi_presets?.length ? p.dpi_presets : [];
    parts.push(`<section class="section"><h3>Pointer speed</h3><p>Sensor resolution in DPI. Higher moves the pointer further for the same hand movement.</p>
      <div class="dpi-readout"><strong id="dpi-val">${cur}</strong><span>DPI</span></div>
      <input class="range" type="range" id="dpi" min="0" max="${sup.length - 1}" step="1" value="${idx}" style="--p:${pct}%" aria-label="Pointer speed">
      <div class="range-scale"><span>${sup[0]}</span><span>${sup[sup.length - 1]}</span></div></section>
      <section class="section"><h3>Speed presets</h3><p>The values a button set to “Cycle pointer speed” steps through. Pick two or more.</p>
      <div class="chips">${suggest.map((v) => `<button class="chip" data-preset="${v}" aria-pressed="${presets.includes(v)}">${v}</button>`).join("")}</div></section>`);
  }
  if (d.wheel || d.smartshift) {
    const rows = [];
    if (d.wheel) {
      rows.push(switchRow("High-resolution scrolling", "Finer, smoother steps in apps that support it.", "hires", p.hires_scroll ?? d.wheel[0]));
      rows.push(switchRow("Reverse wheel direction", `Flips the wheel in hardware, independently of the ${IS_MAC() ? "macOS natural scrolling" : "system scroll direction"} setting.`, "invert", p.invert_wheel ?? d.wheel[1]));
    }
    let ss = "";
    if (d.smartshift) {
      const ratchet = p.smartshift_ratchet ?? d.smartshift[0];
      const th = p.smartshift_threshold ?? d.smartshift[1];
      ss = `<div class="row"><div class="row-text"><div class="row-title">Wheel feel</div><div class="row-sub">Ratchet clicks line by line; free spin glides.</div></div>
        <div class="seg"><button data-ss="ratchet" aria-pressed="${ratchet}">Ratchet</button><button data-ss="free" aria-pressed="${!ratchet}">Free spin</button></div></div>
        <div class="row"><div class="row-text"><div class="row-title">SmartShift sensitivity</div><div class="row-sub">How hard a flick switches to free spin. Right = needs a faster flick.</div></div>
        <input class="range" type="range" id="ss-th" min="1" max="50" value="${Math.min(50, th)}" style="width:200px;--p:${(Math.min(50, th) / 50) * 100}%"></div>`;
    }
    parts.push(`<section class="section"><h3>Scrolling</h3>${rows.join("")}${ss}</section>`);
  }
  return `<div class="pane">${parts.join("") || `<section class="section"><p>Nothing to adjust on this device.</p></section>`}</div>`;
}

const switchRow = (title, sub, key, on) => `<div class="row"><div class="row-text"><div class="row-title">${title}</div><div class="row-sub">${sub}</div></div><button class="switch" role="switch" data-switch="${key}" aria-checked="${!!on}" aria-label="${esc(title)}"></button></div>`;

const DISABLE_KEYS = () => [[1, "Caps Lock"], [2, "Num Lock"], [4, "Scroll Lock"], [8, "Insert"], [16, IS_MAC() ? "⌘ key" : "Windows key"]];

function keyboardHTML(d) {
  const dc = devCfg(d.id);
  const parts = [];
  if (d.backlight || dc.backlight) {
    const b = { ...(d.backlight || {}), ...(dc.backlight || {}) };
    const enabled = b.enabled ?? true;
    const modes = [["auto", "Adaptive", d.backlight?.auto_supported], ["manual", "Fixed level", d.backlight?.manual_supported]].filter((m) => m[2]);
    const mode = modes.some((m) => m[0] === b.mode) ? b.mode : null;
    const levels = d.backlight?.levels || 0;
    parts.push(`<section class="section"><h3>Backlight</h3><p>Key lighting. The keyboard dims by itself when your hands are away${d.features?.some(([id]) => id === "0x1A20") ? " and adapts to the room's light" : ""}.</p>
      ${switchRow("Backlight", enabled ? "On" : "Off — saves battery", "backlight", enabled)}
      ${enabled && modes.length ? `<div class="row"><div class="row-text"><div class="row-title">Brightness</div><div class="row-sub">Adaptive follows the ambient light sensor.</div></div>
        <div class="seg">${modes.map(([k, n]) => `<button data-blmode="${k}" aria-pressed="${mode === k}">${n}</button>`).join("")}</div></div>` : ""}
      ${enabled && mode === "manual" && levels > 1 ? `<div class="row"><div class="row-text"><div class="row-title">Level</div></div>
        <input class="range" type="range" id="bl-level" min="0" max="${levels - 1}" value="${Math.min(b.level ?? 0, levels - 1)}" style="width:220px;--p:${((b.level ?? 0) / (levels - 1)) * 100}%"></div>` : ""}
      ${enabled && !modes.length ? `<p class="help">Brightness is set on the keyboard itself with its backlight keys.</p>` : ""}
    </section>`);
  }
  const fnOn = dc.fn_inverted ?? d.fn_inverted ?? false;
  parts.push(`<section class="section"><h3>Function keys</h3>
    ${d.fn_inverted != null || d.status !== "online" ? switchRow("Use F1, F2… as standard function keys", "When on, hold fn to use the printed media and system actions.", "fninv", fnOn) : `<p>This keyboard doesn't let the computer change its Fn behaviour.</p>`}
  </section>`);
  if (d.disable_keys) {
    const [caps, cur] = d.disable_keys;
    const mask = dc.disabled_keys ?? cur;
    const keys = DISABLE_KEYS().filter(([bit]) => caps & bit);
    if (keys.length) {
      parts.push(`<section class="section"><h3>Disable keys</h3><p>Turned off inside the keyboard, on every computer it's paired with — handy for keys you hit by accident.</p>
        ${keys.map(([bit, n]) => switchRow(`Disable ${n}`, mask & bit ? "Off" : "Works normally", `dk-${bit}`, !!(mask & bit))).join("")}</section>`);
    }
  }
  parts.push(`<section class="section"><h3>Remapping keys</h3><p>Top-row keys can be reassigned in the Keys tab. To remap ordinary keys like F13 on any keyboard, use All keyboards in the sidebar.</p></section>`);
  return `<div class="pane">${parts.join("")}</div>`;
}

function aboutHTML(d) {
  const g = d.guard || {};
  const facts = [
    ["Model", d.id.toUpperCase()],
    ["Connection", d.connection || "—"],
    ["Serial", d.serial || "—"],
    ...d.firmware.map((f) => f.split(": ")),
  ];
  const dc = devCfg(d.id);
  const nb = Object.keys(dc.bindings || {}).length + Object.values(dc.app_bindings || {}).reduce((n, m) => n + Object.keys(m).length, 0);
  return `<div class="pane">
    <section class="section"><h3>Binding guard</h3><p>Logitech devices forget remapped buttons when they sleep, switch channel or reconnect. Keymodus re-applies them on reconnect and checks every 15 seconds.</p>
      <div class="guard-card">
        <div class="stat"><strong>${g.diverted || 0}</strong><span>controls guarded</span></div>
        <div class="stat"><strong>${g.restores || 0}</strong><span>silent restores</span></div>
        <div class="stat"><strong style="font-size:18px">${relTime(g.verified_at)}</strong><span>last verified</span></div>
        <div style="text-align:right"><button class="btn" data-act="activity">Activity</button></div>
      </div></section>
    ${d.host && d.host[0] > 1 ? `<section class="section"><h3>Easy-Switch</h3><p>This ${esc(d.kind)} can pair with ${d.host[0]} computers and is on channel ${d.host[1] + 1} now. Use the numbers at the top right to send it to another one.</p></section>` : ""}
    <section class="section"><h3>Device</h3><dl class="facts">${facts.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("")}</dl></section>
    ${d.features.length ? `<section class="section"><h3>Capabilities</h3><p>HID++ features reported by the firmware.</p><ul class="feature-list">${d.features.map(([id, n]) => `<li><code>${esc(id)}</code>${esc(n || "Unlisted")}</li>`).join("")}</ul></section>` : ""}
    <section class="section"><h3>Restore defaults</h3><p>Clears ${nb ? `all ${nb} assignment${nb === 1 ? "" : "s"}` : "every assignment"} and app profiles for this device. Pointer, backlight and key settings stay. A backup is kept.</p>
      <button class="btn" data-act="reset-device" ${nb ? "" : "disabled"}>Reset buttons</button></section>
    ${d.status === "offline" ? `<section class="section"><h3>Forget this device</h3><p>Removes its saved buttons from Keymodus.</p><button class="btn" data-act="forget">Forget ${esc(d.name)}</button></section>` : ""}
  </div>`;
}

// ---- Smart Actions -----------------------------------------------------------

function renderSmart(view) {
  const all = Object.entries(state.snap.config.smart_actions || {});
  if (state.smartId && !state.snap.config.smart_actions?.[state.smartId]) state.smartId = null;
  if (!state.smartId && all.length) state.smartId = all[0][0];
  const sa = state.smartId ? state.snap.config.smart_actions[state.smartId] : null;
  const used = (id) => {
    let n = 0;
    for (const dc of Object.values(state.snap.config.devices)) {
      for (const a of Object.values(dc.bindings || {})) if (a.type === "smart_action" && a.id === id) n++;
      for (const m of Object.values(dc.app_bindings || {})) for (const a of Object.values(m)) if (a.type === "smart_action" && a.id === id) n++;
    }
    return n;
  };
  const list = `<div class="panel-scroll"><div class="panel-title"><span>Saved</span></div>
      <ul class="control-list">${all.map(([id, x], i) => `<li><button class="control" data-smart="${esc(id)}" data-bound="${state.smartId === id}">
        <span class="control-num">${i + 1}</span><span><div class="control-name">${esc(x.name)}</div><div class="control-action">${x.steps.length} step${x.steps.length === 1 ? "" : "s"}${used(id) ? ` · on ${used(id)} button${used(id) === 1 ? "" : "s"}` : ""}</div></span></button></li>`).join("")}</ul>
      <button class="btn btn-accent" data-act="new-smart" style="width:100%;margin-top:12px">New Smart Action</button></div>`;

  let editor = `<div class="empty"><div class="empty-inner"><h2>Chain shortcuts together</h2><p>A Smart Action runs several steps with one press: press a shortcut, wait, open an app or a link, type text. Create one, then assign it to any button or key.</p><button class="btn btn-accent" data-act="new-smart">New Smart Action</button></div></div>`;
  if (sa) {
    const steps = sa.steps.map((st, i) => `<li class="step"><span class="control-num">${i + 1}</span>
        <span class="step-what">${st.kind === "delay" ? `Wait <input class="input step-ms" type="number" min="0" max="10000" step="50" value="${st.ms}" data-step-ms="${i}"> ms` : actionHTML(st.action)}</span>
        <span class="step-tools"><button class="icon-btn" data-step-up="${i}" ${i ? "" : "disabled"} aria-label="Move up">↑</button><button class="icon-btn" data-step-down="${i}" ${i < sa.steps.length - 1 ? "" : "disabled"} aria-label="Move down">↓</button><button class="icon-btn" data-step-del="${i}" aria-label="Remove step">${CLOSE}</button></span></li>`).join("");
    const addCats = [["popular", "Popular"], ["keys", "Keystroke"], ["navigation", "Navigation"], ["media", "Media"], ["system", "System"], ["mouse", "Mouse"], ["open", "Open"], ["text", "Text"]];
    editor = `<div class="pane smart-pane">
      <section class="section"><div class="field"><label for="sa-name">Name</label><input class="input" id="sa-name" value="${esc(sa.name)}" maxlength="60" spellcheck="false"></div></section>
      <section class="section"><h3>Steps</h3>
        ${sa.steps.length ? `<ol class="steps">${steps}</ol>` : `<p class="help">No steps yet. Add the first one below.</p>`}
        <div class="banner-actions" style="margin-top:12px;justify-content:flex-start">
          <button class="btn" data-act="add-delay" ${sa.steps.length >= 32 ? "disabled" : ""}>Add pause</button>
          <button class="btn" data-act="try-smart" ${sa.steps.length ? "" : "disabled"}>Try it</button>
          <span class="spacer"></span>
          <button class="btn btn-ghost" data-act="delete-smart">Delete</button>
        </div></section>
      <section class="section"><h3>Add a step</h3>
        <div class="cats" role="toolbar">${addCats.map(([k, n]) => `<button class="cat" data-smart-cat="${k}" aria-pressed="${(state.smartAdd ?? "popular") === k}">${n}</button>`).join("")}</div>
        ${pickerHTML(state.smartAdd ?? "popular", null, { kind: "step" })}
      </section></div>`;
  }
  view.innerHTML = `
    <header class="head"><div class="head-text"><div class="eyebrow"><span>Library</span><span class="sep"></span><span>Works with every device</span></div><h1 class="title">Smart Actions</h1></div></header>
    <div class="tabs"><button class="tab" aria-selected="true">Sequences</button><span class="tab-ink"></span></div>
    <div class="workbench smart-bench"><section class="panel">${list}</section><section class="stage smart-stage">${editor}</section></div>`;
  placeInk(false);
  wireRecorder();
}

async function saveSmart(patch) {
  const id = state.smartId;
  const cur = state.snap.config.smart_actions?.[id] ?? { name: "Smart Action", steps: [] };
  const next = { ...cur, ...patch };
  const before = new Set(Object.keys(state.snap.config.smart_actions || {}));
  state.snap.config = await call("save_smart_action", { id, action: next });
  if (!id) state.smartId = Object.keys(state.snap.config.smart_actions).find((k) => !before.has(k)) ?? null;
  render();
}

function renderEmpty(view) {
  view.innerHTML = `<div class="empty"><div class="empty-inner">
    <svg class="empty-art" viewBox="0 0 120 120" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square"><rect x="38" y="16" width="44" height="74" rx="22"/><path d="M60 16v24"/><rect x="56" y="26" width="8" height="12" fill="var(--volt)" stroke="none"/><path d="M14 104h22M46 104h8M64 104h42" opacity=".5"/></svg>
    <h2>Wake up a mouse or keyboard</h2>
    <p>Logitech devices on Bluetooth, Bolt or Unifying appear here by themselves. For other brands, use All mice or All keyboards.</p>
    <button class="btn" data-act="bt">Bluetooth settings</button>
  </div></div>`;
}

// ---- Generic devices ----------------------------------------------------------

function renderGeneric(view) {
  const kind = state.sel.kind;
  if (state.snap.capabilities?.any_device === false) {
    view.innerHTML = `<header class="head"><div class="head-text"><div class="eyebrow"><span>Any brand</span></div><h1 class="title">${kind === "mouse" ? "All mice" : "All keyboards"}</h1></div></header>
      <div class="empty"><div class="empty-inner"><h2>Not available on this system yet</h2><p>Remapping buttons on any brand needs a system-wide input hook. Keymodus has one on macOS and Windows; on Linux it's planned. Logitech devices in the sidebar work normally.</p></div></div>`;
    return;
  }
  const g = state.snap.config.generic;
  const isMouse = kind === "mouse";
  const map = isMouse ? g.mouse_buttons || {} : g.keys || {};
  const codes = new Set(Object.keys(map));
  if (isMouse) state.discovered.forEach((b) => codes.add(String(b)));
  const list = [...codes].sort((a, b) => (isMouse ? a - b : a.localeCompare(b)));
  const label = (c) => (isMouse ? ({ 2: "Middle button", 3: "Back button", 4: "Forward button" }[c] ?? `Button ${+c + 1}`) : `${keyLabel(c)} key`);

  let panel;
  if (state.editing != null) {
    const code = String(state.editing);
    panel = editorHTML({ kind: "generic", gkind: kind, code, name: label(code), gesture: false, hasDpi: false, action: map[code] ?? { type: "default" }, override: false });
  } else {
    panel = `<div class="panel-scroll"><div class="panel-title"><span>${isMouse ? "Buttons" : "Keys"}</span></div>
      ${list.length ? `<ul class="control-list">${list.map((c, i) => `<li><button class="control" data-edit="${esc(c)}" data-bound="${isBound(map[c])}">
          <span class="control-num">${i + 1}</span><span><div class="control-name">${esc(label(c))}</div><div class="control-action">${isBound(map[c]) ? actionHTML(map[c]) : "Default"}</div></span></button></li>`).join("")}</ul>`
        : `<p class="help" style="padding:4px 6px">${isMouse ? "Nothing yet — press an extra button on any mouse." : "Nothing yet — add a key on the left."}</p>`}
    </div>`;
  }
  view.innerHTML = `
    <header class="head"><div class="head-text"><div class="eyebrow"><span>Any brand</span><span class="sep"></span><span>USB, Bluetooth or receiver</span></div><h1 class="title">${isMouse ? "All mice" : "All keyboards"}</h1></div></header>
    <div class="tabs"><button class="tab" aria-selected="true">${isMouse ? "Buttons" : "Keys"}</button><span class="tab-ink"></span></div>
    <div class="workbench">
      <section class="stage" style="place-items:start center;padding-top:56px"><div class="hud" aria-hidden="true"><i></i><i></i><i></i><i></i><b class="hud-tl">Bus ${isMouse ? "Mouse" : "Keyboard"} / any vendor</b><b class="hud-br">Bound <em>${pad(Object.values(map).filter(isBound).length)}</em></b></div><div style="width:min(100%,460px);position:relative;z-index:2">
        ${isMouse
          ? `<div class="listen"><span class="pulse"></span><div><div class="row-title">Press an extra button on any mouse</div><div class="row-sub">Middle, back, forward and side buttons show up here. Left and right clicks are never touched.</div></div></div>`
          : `<div class="field"><label>Add a key</label><div class="recorder" tabindex="0" data-recorder data-addkey><span class="keys kbd-lg"><kbd>F13</kbd></span><span class="prompt">Click here, then press the key</span></div>
             <p class="help" style="margin-top:8px">Applies when pressed without modifiers, on every keyboard. Best for keys you rarely type with: F13–F20, Home, End.</p></div>`}
        ${state.snap.permissions.accessibility ? "" : `<p class="help">Needs Accessibility access to see other devices.</p>`}
      </div></section>
      <section class="panel">${panel}</section>
    </div>`;
  placeInk(false);
  wireRecorder();
}

// ---------------------------------------------------------------------------
// Sheets
// ---------------------------------------------------------------------------

function openSheet(kind) {
  state.sheet = kind;
  const sheet = $("#sheet");
  sheet.removeAttribute("data-closing");
  sheet.hidden = false;
  $("#scrim").hidden = false;
  renderSheet();
}

function closeSheet() {
  const sheet = $("#sheet");
  if (sheet.hidden) return;
  sheet.setAttribute("data-closing", "");
  $("#scrim").hidden = true;
  state.sheet = null;
  setTimeout(() => { if (!state.sheet) sheet.hidden = true; }, 170);
}

function renderSheet() {
  const s = state.snap;
  const sheet = $("#sheet");
  if (state.sheet === "activity") {
    sheet.innerHTML = `<div class="sheet-head"><div><div class="sheet-kicker">Guard / log</div><h2>Activity</h2></div><button class="icon-btn" data-act="close-sheet" aria-label="Close">${CLOSE}</button></div>
      <div class="sheet-body"><p class="help" style="margin-bottom:10px">What the binding guard has done. Restores are the moments Options+ would have “forgotten” your buttons.</p>
      <ul class="log">${(s.guard_log || []).map((e) => `<li><span class="t" data-tone="${e.tone}"></span><span><span class="dev">${esc(e.device)}</span> <span class="msg">${esc(e.message)}</span></span><time>${relTime(e.at)}</time></li>`).join("") || `<li><span></span><span class="msg">Nothing yet.</span><span></span></li>`}</ul>
      <div style="margin-top:14px"><button class="btn btn-ghost" data-act="logs">Show log file</button></div></div>`;
  } else if (state.sheet === "settings") {
    const th = s.config.settings.gesture_threshold;
    sheet.innerHTML = `<div class="sheet-head"><div><div class="sheet-kicker">System / v${esc(s.version)}</div><h2>Settings</h2></div><button class="icon-btn" data-act="close-sheet" aria-label="Close">${CLOSE}</button></div>
      <div class="sheet-body">
        ${switchRow("Open at login", `Starts quietly in the ${IS_MAC() ? "menu bar" : "system tray"} so your buttons work right away.`, "login", s.launch_at_login)}
        <div class="row"><div class="row-text"><div class="row-title">Updates</div><div class="row-sub" id="upd-status">${s.update ? `Version ${esc(s.update.version)} is ready to install.` : `You're on ${esc(s.version)}. Releases come from github.com/aflorea4/keymodus and are signature-checked.`}</div></div>
          ${s.update ? `<button class="btn btn-accent" data-act="install-update">Install</button>` : `<button class="btn" data-act="check-update">Check now</button>`}</div>
        ${switchRow("Check for updates automatically", "Looks for a new release at launch and every 6 hours. Nothing is installed without your click.", "autoupd", s.config.settings.auto_update_check !== false)}
        <div class="row" style="display:block"><div class="row-title">Gesture distance</div><div class="row-sub" style="margin-bottom:6px">How far to move before a held gesture button counts as a swipe.</div>
          <input class="range" type="range" id="g-th" min="20" max="200" value="${th}" style="--p:${((th - 20) / 180) * 100}%"></div>
        ${s.capabilities?.options_plus === false ? "" : `<div class="row"><div class="row-text"><div class="row-title">Logi Options+</div><div class="row-sub">${s.options_plus.installed ? (s.options_plus.running ? "Running" : "Installed, paused") : "Not installed"}</div></div>
          ${s.options_plus.installed ? `<button class="btn" data-act="import">Import setup</button>` : ""}
          ${s.options_plus.installed ? (s.options_plus.running ? `<button class="btn" data-act="pause-op">Pause</button>` : `<button class="btn" data-act="resume-op">Resume</button>`) : ""}</div>`}
        <div class="row"><div class="row-text"><div class="row-title">Settings file</div><div class="row-sub">Move your setup to another computer, or keep a copy.</div></div>
          <button class="btn" data-act="export">Export</button><label class="btn" style="cursor:pointer">Import<input type="file" accept="application/json,.json" id="import-file" hidden></label></div>
        <div class="row" style="display:block"><div class="row-title">Backups</div><div class="row-sub" style="margin-bottom:6px">Every change keeps the previous version. Restore one if something went wrong.</div>
          ${(s.backups || []).slice(0, 8).map((b) => `<div class="row" style="padding:6px 0"><span class="row-text" style="font:12px var(--font-mono);color:var(--text-2)">${esc(prettyBackup(b))}</span><button class="btn btn-ghost" data-act="restore" data-name="${esc(b)}">Restore</button></div>`).join("") || `<p class="help">No backups yet.</p>`}
        </div>
        <p class="help" style="margin-top:22px;font:500 9.5px/1.6 var(--font-mono);text-transform:uppercase;letter-spacing:.08em">Keymodus v${esc(s.version)} / <button class="linkish" data-act="reveal-config">${esc(s.config_dir || "")}</button> / MIT / <button class="linkish" data-act="open-repo">Source</button></p>
      </div>`;
  }
}

function prettyBackup(name) {
  const m = name.match(/config-(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})/);
  if (!m) return name;
  const d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]));
  return d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

function toast(html) {
  const el = document.createElement("div");
  el.className = "toast";
  el.innerHTML = `<span class="dot"></span><span>${html}</span>`;
  const host = $("#toaster");
  host.appendChild(el);
  while (host.children.length > 3) host.firstChild.remove();
  setTimeout(() => { el.setAttribute("data-leaving", ""); setTimeout(() => el.remove(), 200); }, 2200);
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

async function call(cmd, args) {
  try {
    return await backend.invoke(cmd, args);
  } catch (e) {
    toast(esc(String(e)));
    throw e;
  }
}

async function assign(action, slot = null) {
  if (state.sel?.type === "smart") {
    const sa = state.snap.config.smart_actions?.[state.smartId];
    if (!sa || slot || sa.steps.length >= 32) return;
    await saveSmart({ steps: [...sa.steps, { kind: "action", action }] });
    toast(`Step added: ${actionHTML(action)}`);
    return;
  }
  const d = currentDevice();
  if (state.sel?.type === "generic") {
    const code = String(state.editing);
    const cur = state.snap.config.generic[state.sel.kind === "mouse" ? "mouse_buttons" : "keys"][code];
    const next = slot ? { ...(cur?.type === "gesture" ? cur : EMPTY_GESTURE()), [slot]: action } : action;
    state.snap.config = await call("set_generic_binding", { kind: state.sel.kind, code, action: next });
  } else if (d) {
    const cur = bindingFor(d.id, state.editing).action;
    const next = slot ? { ...(cur.type === "gesture" ? cur : EMPTY_GESTURE()), [slot]: action } : action;
    state.snap.config = await call("set_binding", { device: d.id, cid: state.editing, app: state.scope, action: next });
    const c = d.controls.find((x) => x.cid === state.editing);
    toast(`${esc(c?.name ?? "Button")} → ${actionHTML(slot ? action : next)}`);
  }
  render();
}

async function savePointer(patch) {
  const d = currentDevice();
  if (!d) return;
  const pointer = { ...(devCfg(d.id).pointer || {}), ...patch };
  state.snap.config = await call("set_pointer", { device: d.id, pointer });
}

async function refresh() {
  state.snap = await backend.invoke("get_state");
  document.documentElement.dataset.os = OS();
  $("#brand-ver").textContent = "v" + state.snap.version;
  if (!state.sel && !T) {
    // Browser preview only: ?sel=<device id>|smart|mouse|key&tab=<tab>&edit=<cid>&sheet=settings
    const q = new URLSearchParams(location.search);
    const sel = q.get("sel");
    if (sel) state.sel = sel === "smart" ? { type: "smart" } : sel === "mouse" || sel === "key" ? { type: "generic", kind: sel } : { type: "device", id: sel };
    if (q.get("tab")) state.tab = q.get("tab");
    if (q.get("edit")) { state.editing = +q.get("edit"); state.cat = q.get("cat"); }
    if (q.get("sheet")) setTimeout(() => openSheet(q.get("sheet")), 0);
  }
  if (!state.sel) {
    const first = state.snap.devices.find((d) => d.status !== "offline") ?? state.snap.devices[0];
    state.sel = first ? { type: "device", id: first.id } : { type: "generic", kind: "mouse" };
  }
  render();
  if (state.sheet) renderSheet();
}

async function loadApps() {
  if (state.apps) return;
  state.apps = await backend.invoke("list_apps");
  if (state.popover) render();
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

document.addEventListener("click", async (e) => {
  const t = e.target.closest("button, [data-act]");
  if (!t) {
    if (state.popover && !e.target.closest(".popover")) { state.popover = false; render(); }
    return;
  }
  const ds = t.dataset;

  if (ds.dev) { state.sel = { type: "device", id: ds.dev }; state.editing = null; state.scope = null; state.popover = false; state.confirmHost = null; state.query = ""; state.firstListRender = true; return render(); }
  if (ds.smartNav) { state.sel = { type: "smart" }; state.editing = null; state.popover = false; return render(); }
  if (ds.smart) { state.smartId = ds.smart; return render(); }
  if (ds.smartCat) { state.smartAdd = ds.smartCat; return render(); }
  if (ds.host !== undefined) { state.confirmHost = +ds.host; return render(); }
  if (ds.removeProfile) {
    const d = currentDevice();
    if (devCfg(d.id).app_bindings?.[ds.removeProfile]) state.snap.config = await call("remove_app_profile", { device: d.id, app: ds.removeProfile });
    if (state.scope === ds.removeProfile) state.scope = null;
    return render();
  }
  if (ds.stepUp !== undefined || ds.stepDown !== undefined || ds.stepDel !== undefined) {
    const steps = [...state.snap.config.smart_actions[state.smartId].steps];
    const i = +(ds.stepUp ?? ds.stepDown ?? ds.stepDel);
    if (ds.stepDel !== undefined) steps.splice(i, 1);
    else { const j = ds.stepUp !== undefined ? i - 1 : i + 1; [steps[i], steps[j]] = [steps[j], steps[i]]; }
    return saveSmart({ steps });
  }
  if (ds.blmode) return saveBacklight({ mode: ds.blmode });
  if (ds.generic) { state.sel = { type: "generic", kind: ds.generic }; state.editing = null; state.popover = false; return render(); }
  if (ds.tab) { state.tab = ds.tab; state.editing = null; state.confirmHost = null; render(); return; }
  if (ds.edit !== undefined) {
    const v = state.sel?.type === "generic" ? ds.edit : +ds.edit;
    state.editing = v; state.cat = null; state.gestureSlot = "click"; state.gestureCat = null; state.query = "";
    if (state.sel?.type === "device") loadApps();
    return render();
  }
  if (ds.cat) { state.cat = ds.cat; return render(); }
  if (ds.gslot) { state.gestureSlot = ds.gslot; state.gestureCat = null; return render(); }
  if (ds.gcat) { state.gestureCat = ds.gcat; return render(); }
  if (ds.pick) return assign(JSON.parse(ds.pick), ds.slot || null);
  if (ds.scope !== undefined) { state.scope = ds.scope || null; state.popover = false; state.appQuery = ""; return render(); }
  if (ds.preset) {
    const d = currentDevice(); const v = +ds.preset;
    const cur = new Set(devCfg(d.id).pointer?.dpi_presets || []);
    cur.has(v) ? cur.delete(v) : cur.add(v);
    await savePointer({ dpi_presets: [...cur].sort((a, b) => a - b) });
    return render();
  }
  if (ds.switch) return onSwitch(t, ds.switch);
  if (ds.ss) { await savePointer({ smartshift_ratchet: ds.ss === "ratchet" }); return render(); }

  switch (ds.act) {
    case "scope": state.popover = !state.popover; state.appQuery = ""; if (state.popover) loadApps(); render(); return;
    case "host-cancel": state.confirmHost = null; return render();
    case "host-go": {
      const d = currentDevice(); const h = state.confirmHost; state.confirmHost = null;
      await call("switch_host", { device: d.id, host: h });
      toast(`${esc(d.name)} → channel ${h + 1}`);
      return render();
    }
    case "new-smart": {
      state.sel = { type: "smart" }; state.editing = null; state.smartId = null; state.smartAdd = "popular";
      const n = Object.keys(state.snap.config.smart_actions || {}).length + 1;
      return saveSmart({ name: `Smart Action ${n}`, steps: [] });
    }
    case "add-delay": { const sa = state.snap.config.smart_actions[state.smartId]; return saveSmart({ steps: [...sa.steps, { kind: "delay", ms: 300 }] }); }
    case "try-smart": toast("Running in a moment…"); return call("test_steps", { steps: state.snap.config.smart_actions[state.smartId].steps });
    case "delete-smart": {
      if (!t.dataset.armed) { t.dataset.armed = "1"; t.textContent = "Click again to delete"; setTimeout(() => { if (t.isConnected) { delete t.dataset.armed; t.textContent = "Delete"; } }, 2500); return; }
      state.snap.config = await call("delete_smart_action", { id: state.smartId });
      state.smartId = null; toast("Smart Action deleted"); return render();
    }
    case "reset-device": {
      if (!t.dataset.armed) { t.dataset.armed = "1"; t.textContent = "Click again to reset"; setTimeout(() => { if (t.isConnected) { delete t.dataset.armed; t.textContent = "Reset buttons"; } }, 2500); return; }
      const d = currentDevice(); state.snap.config = await call("reset_device", { device: d.id }); state.scope = null;
      toast("Buttons reset to default"); return render();
    }
    case "check-update": {
      t.disabled = true; t.textContent = "Checking…";
      try {
        const u = await call("check_update");
        state.snap.update = u;
        toast(u ? `Keymodus ${esc(u.version)} is available` : "You're up to date");
      } finally { render(); if (state.sheet) renderSheet(); }
      return;
    }
    case "install-update": state.updateProgress = { done: 0, total: null }; render(); try { await call("install_update"); } catch { state.updateProgress = null; render(); } return;
    case "release-notes": return call("open_repo", { path: "releases/latest" });
    case "open-repo": return call("open_repo", { path: "" });
    case "reveal-config": return call("reveal_config");
    case "export": { const path = await call("export_config"); if (path) toast(`Saved to ${esc(path.split(/[\\/]/).pop())}`); return; }
    case "close-editor": state.editing = null; return render();
    case "set-default": return assign({ type: "default" });
    case "clear-override": {
      const d = currentDevice();
      state.snap.config = await call("clear_app_binding", { device: d.id, cid: state.editing, app: state.scope });
      return render();
    }
    case "make-gesture": return assign(EMPTY_GESTURE());
    case "try": {
      const d = currentDevice();
      const a = state.sel?.type === "generic" ? state.snap.config.generic[state.sel.kind === "mouse" ? "mouse_buttons" : "keys"][state.editing] : bindingFor(d.id, state.editing).action;
      toast("Running in a moment…");
      return call("test_action", { action: a });
    }
    case "dismiss": await call("dismiss_notice", { id: ds.id }); return refresh();
    case "perm": await call("request_permission", { kind: ds.kind }); return;
    case "pause-op":
      t.disabled = true;
      try { await call("pause_options_plus"); toast("Options+ paused until next login"); } finally { await refresh(); }
      return;
    case "resume-op": await call("resume_options_plus"); toast("Options+ started"); return refresh();
    case "import-later": prefs.set("importDismissed", true); return render();
    case "import": {
      const r = await call("import_options_plus");
      state.snap.config = r.config;
      prefs.set("importDismissed", true);
      const rep = r.report;
      toast(rep.imported ? `Imported ${rep.imported} assignment${rep.imported > 1 ? "s" : ""}${rep.apps.length ? ` incl. ${rep.apps.length} app profile${rep.apps.length > 1 ? "s" : ""}` : ""}` : "Options+ had only default assignments");
      return refresh();
    }
    case "activity": return openSheet("activity");
    case "close-sheet": return closeSheet();
    case "logs": return call("reveal_logs");
    case "bt": return call("open_settings", { pane: "bluetooth" });
    case "restore": state.snap.config = await call("restore_backup", { name: ds.name }); toast("Settings restored"); return refresh();
    case "forget": {
      const d = currentDevice();
      state.snap.config = await call("forget_device", { device: d.id });
      state.sel = null;
      return refresh();
    }
  }
});

$("#guard-pill").addEventListener("click", () => openSheet("activity"));
$("#settings-btn").addEventListener("click", () => openSheet("settings"));
$("#scrim").addEventListener("click", closeSheet);

async function onSwitch(el, key) {
  const on = el.getAttribute("aria-checked") !== "true";
  el.setAttribute("aria-checked", on);
  const d = currentDevice();
  if (key === "hires") await savePointer({ hires_scroll: on });
  else if (key === "invert") await savePointer({ invert_wheel: on });
  else if (key === "fninv") state.snap.config = await call("set_fn_inverted", { device: d.id, on });
  else if (key === "login") { await call("set_launch_at_login", { on }); state.snap.launch_at_login = on; }
  else if (key === "autoupd") { await call("set_auto_update", { on }); state.snap.config.settings.auto_update_check = on; }
  else if (key === "backlight") { await saveBacklight({ enabled: on }); }
  else if (key.startsWith("dk-")) {
    const bit = +key.slice(3);
    const cur = devCfg(d.id).disabled_keys ?? d.disable_keys?.[1] ?? 0;
    state.snap.config = await call("set_disabled_keys", { device: d.id, mask: on ? cur | bit : cur & ~bit });
    render();
  }
}

async function saveBacklight(patch) {
  const d = currentDevice();
  if (!d) return;
  const base = { enabled: d.backlight?.enabled ?? true, mode: d.backlight?.mode ?? "auto", level: d.backlight?.level ?? 0 };
  const backlight = { ...base, ...(devCfg(d.id).backlight || {}), ...patch };
  state.snap.config = await call("set_backlight", { device: d.id, backlight });
  render();
}

let dpiTimer;
document.addEventListener("input", (e) => {
  const t = e.target;
  if (t.id === "dpi") {
    const d = currentDevice();
    const v = d.dpi.supported[+t.value];
    t.style.setProperty("--p", `${(t.value / t.max) * 100}%`);
    $("#dpi-val").textContent = v;
    clearTimeout(dpiTimer);
    dpiTimer = setTimeout(() => savePointer({ dpi: v }), 220);
  } else if (t.id === "ss-th") {
    t.style.setProperty("--p", `${(t.value / 50) * 100}%`);
    clearTimeout(dpiTimer);
    dpiTimer = setTimeout(() => savePointer({ smartshift_threshold: +t.value }), 220);
  } else if (t.id === "g-th") {
    t.style.setProperty("--p", `${((t.value - 20) / 180) * 100}%`);
    clearTimeout(dpiTimer);
    dpiTimer = setTimeout(() => call("set_gesture_threshold", { px: +t.value }), 300);
  } else if (t.id === "bl-level") {
    t.style.setProperty("--p", `${(t.value / t.max) * 100}%`);
    clearTimeout(dpiTimer);
    dpiTimer = setTimeout(() => saveBacklight({ level: +t.value, mode: "manual" }), 220);
  } else if (t.id === "action-search") {
    state.query = t.value;
    const pos = t.selectionStart;
    render();
    const n = $("#action-search");
    n?.focus({ preventScroll: true });
    n?.setSelectionRange(pos, pos);
  } else if (t.id === "app-search") {
    state.appQuery = t.value;
    const pos = t.selectionStart;
    render();
    const n = $("#app-search");
    n?.focus({ preventScroll: true });
    n?.setSelectionRange(pos, pos);
  }
});

// Free-form fields save on Enter / ⌘↩ / blur.
async function commitField(el) {
  const v = el.value.trim();
  const kind = el.dataset.field;
  if (!v) return;
  const action = kind === "launch_app" ? { type: "launch_app", path: v } : kind === "open_url" ? { type: "open_url", url: /^\w+:/.test(v) ? v : "https://" + v } : kind === "shell" ? { type: "shell", command: v } : { type: "type_text", text: el.value };
  await assign(action, el.dataset.slot || null);
}
document.addEventListener("keydown", (e) => {
  const f = e.target.closest?.("[data-field]");
  if (f && e.key === "Enter" && (f.tagName === "INPUT" || e.metaKey || e.ctrlKey)) { e.preventDefault(); commitField(f); }
});
document.addEventListener("change", async (e) => {
  const f = e.target.closest?.("[data-field]");
  if (f) return commitField(f);
  const t = e.target;
  if (t.id === "sa-name") return saveSmart({ name: t.value.trim() || "Smart Action" });
  if (t.dataset?.stepMs !== undefined) {
    const steps = [...state.snap.config.smart_actions[state.smartId].steps];
    steps[+t.dataset.stepMs] = { kind: "delay", ms: Math.max(0, Math.min(10000, Math.round(+t.value || 0))) };
    return saveSmart({ steps });
  }
  if (t.id === "import-file" && t.files?.[0]) {
    const file = t.files[0];
    if (file.size > 4 * 1024 * 1024) return toast("That file is too large.");
    const json = await file.text();
    state.snap.config = await call("import_config", { json });
    toast("Settings imported — the previous ones are in Backups");
    return refresh();
  }
});

// Shortcut recorder.
const MOD_CODES = new Set(["MetaLeft", "MetaRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "ControlLeft", "ControlRight", "CapsLock", "Fn"]);
function wireRecorder() {
  for (const r of document.querySelectorAll("[data-recorder]")) {
    r.addEventListener("keydown", async (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (MOD_CODES.has(e.code)) return;
      if (r.hasAttribute("data-addkey")) {
        state.editing = e.code;
        state.cat = null;
        return render();
      }
      const mods = [];
      if (e.ctrlKey) mods.push("ctrl");
      if (e.altKey) mods.push("option");
      if (e.shiftKey) mods.push("shift");
      if (e.metaKey) mods.push("cmd");
      await assign({ type: "keystroke", key: e.code, mods }, r.dataset.slot || null);
      $("[data-recorder]")?.focus({ preventScroll: true });
    });
  }
}

// Global keys: Esc closes things; ⌘W hides the window (no native accelerator, see main.rs).
document.addEventListener("keydown", (e) => {
  if (e.target.closest?.("[data-recorder]")) return;
  if (e.key === "Escape") {
    if (state.sheet) return closeSheet();
    if (state.popover) { state.popover = false; return render(); }
    if (state.confirmHost != null) { state.confirmHost = null; return render(); }
    if (state.query && e.target.id === "action-search") { state.query = ""; return render(); }
    if (state.editing != null && !e.target.closest?.("input, textarea")) { state.editing = null; return render(); }
  }
  if ((IS_MAC() ? e.metaKey : e.ctrlKey) && e.code === "KeyW" && T) { e.preventDefault(); T.window.getCurrentWindow().hide(); }
});

window.addEventListener("resize", () => placeInk(false));

// ---------------------------------------------------------------------------
// Live events from the backend
// ---------------------------------------------------------------------------

function devicesKey(ds) {
  return JSON.stringify(ds.map((d) => ({ ...d, guard: { ...d.guard, verified_at: 0 } })));
}

backend.listen("devices", (ds) => {
  if (!state.snap) return;
  state.snap.devices = ds;
  const key = devicesKey(ds);
  const busy = document.activeElement?.matches?.("input, textarea, [data-recorder]");
  if (key !== state.lastDevicesKey && !busy) {
    state.lastDevicesKey = key;
    if (state.sel?.type === "device" && !ds.some((d) => d.id === state.sel.id)) state.sel = null;
    if (!state.sel && ds.length) state.sel = { type: "device", id: ds[0].id };
    render();
  } else {
    renderGuardPill();
  }
});

backend.listen("guard", (entry) => {
  if (!state.snap) return;
  state.snap.guard_log = [entry, ...(state.snap.guard_log || [])].slice(0, 200);
  renderGuardPill();
  if (state.sheet === "activity") renderSheet();
});

backend.listen("press", ({ device, cid, down }) => {
  if (!down || state.sel?.type !== "device" || state.sel.id !== device) return;
  const ring = document.querySelector(`.spot[data-cid="${cid}"] .spot-ring`);
  ring?.animate([{ opacity: 0.9, transform: "scale(0.55)" }, { opacity: 0, transform: "scale(1.9)" }], { duration: 520, easing: "cubic-bezier(0.23, 1, 0.32, 1)" });
  const row = document.querySelector(`.control[data-cid="${cid}"]`);
  if (row) {
    row.dataset.hit = "true";
    setTimeout(() => delete row.dataset.hit, 360);
  }
});

backend.listen("gesture", ({ dir }) => {
  const cell = document.querySelector(`.g-cell[data-gslot="${dir}"]`);
  if (cell) { cell.dataset.flash = "true"; setTimeout(() => delete cell.dataset.flash, 400); }
});

backend.listen("dpi", ({ dpi }) => toast(`Pointer speed ${dpi} DPI`));

backend.listen("update", (info) => {
  if (!state.snap) return;
  state.snap.update = info;
  renderBanners();
  if (state.sheet === "settings") renderSheet();
});

backend.listen("update-progress", (p) => {
  state.updateProgress = p;
  renderBanners();
});

backend.listen("generic", ({ kind, button }) => {
  if (kind !== "mouse" || button < 2) return;
  const fresh = !state.discovered.has(button);
  state.discovered.add(button);
  if (state.sel?.type === "generic" && state.sel.kind === "mouse" && fresh && state.editing == null) render();
});

// Keep relative times fresh without re-rendering everything.
setInterval(() => { if (state.sheet === "activity") renderSheet(); }, 15000);
window.addEventListener("focus", () => refresh());

refresh();

// ---------------------------------------------------------------------------
// Schematics for devices without artwork
// ---------------------------------------------------------------------------

function schematicMouse() {
  return `<svg class="schematic" viewBox="0 0 144 200" fill="none" stroke="currentColor" style="color:var(--text-3)" stroke-width="1.4" stroke-linecap="round">
    <path d="M72 6c-33 0-56 22-56 62v58c0 40 23 68 56 68s56-28 56-68V68C128 28 105 6 72 6Z" fill="var(--surface)"/>
    <path d="M72 6v62M16 68h112" opacity=".5"/>
    <rect x="66" y="26" width="12" height="26" rx="6" fill="var(--sunken)"/>
    <path d="M16 84c-6 2-8 10-6 20M16 108c-6 2-8 10-6 20" opacity=".6"/>
  </svg>`;
}

function schematicKeyboard() {
  const keys = [];
  for (let r = 0; r < 5; r++) for (let c = 0; c < 14; c++) keys.push(`<rect x="${14 + c * 22.6}" y="${46 + r * 20}" width="18" height="15" rx="3"/>`);
  const frow = Array.from({ length: 14 }, (_, c) => `<rect x="${14 + c * 22.6}" y="20" width="18" height="12" rx="3"/>`).join("");
  return `<svg class="schematic" viewBox="0 0 340 150" fill="none" stroke="currentColor" style="color:var(--text-3)" stroke-width="1">
    <rect x="4" y="8" width="332" height="138" rx="12" fill="var(--surface)" stroke-width="1.4"/>
    <g opacity=".7">${frow}</g><g opacity=".35">${keys.join("")}</g></svg>`;
}
