//! Device engine.
//!
//! One supervisor thread watches the HID bus; each Logitech HID++ interface gets
//! a worker thread that owns it. Workers identify the device, divert the
//! controls the user has bound, translate button events into actions, and run
//! the *binding guard*:
//!
//! * diversion is volatile on the device, so it is re-applied on every
//!   (re)connection — including the 0x1D4B "please reconfigure me" event;
//! * every 15 s, and right after the device wakes from idle, the guard reads
//!   back each diverted control and silently restores any the device dropped;
//! * a device that stops answering is shown as asleep — never as an error, and
//!   its configuration is never touched.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::CString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hidpp::features::{self as f, Control, ControlEvent, DeviceKind};
use hidpp::{Device, Message, Transport};
use logi_core::catalog::{Catalog, DeviceArt};
use logi_core::{Action, Config, DeviceConfig, Gesture, LoadOutcome, Store};
use serde::Serialize;

use crate::actions;
use crate::platform::{self, TapEvent};

const VERIFY_EVERY: Duration = Duration::from_secs(15);
const BATTERY_EVERY: Duration = Duration::from_secs(300);
const IDLE_WAKE: Duration = Duration::from_secs(20);
const RETRY_ASLEEP: Duration = Duration::from_secs(4);
const RETRY_EMPTY_SLOT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// What the UI sees
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ControlView {
    pub cid: u16,
    pub name: String,
    pub gesture: bool,
    pub mouse: bool,
    /// F-key number for top-row keys on keyboards (1 = F1).
    pub fkey: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacklightView {
    pub enabled: bool,
    /// "basic" | "auto" | "manual"
    pub mode: String,
    pub level: u8,
    pub levels: u8,
    pub auto_supported: bool,
    pub manual_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtView {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub hotspots: Vec<logi_core::catalog::Hotspot>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DpiView {
    pub current: u16,
    pub supported: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GuardView {
    pub diverted: usize,
    pub verified_at: Option<u64>,
    pub restores: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub connection: String,
    /// "online" | "asleep" | "offline"
    pub status: String,
    pub battery: Option<BatteryView>,
    pub controls: Vec<ControlView>,
    pub art: Option<ArtView>,
    pub features: Vec<(String, String)>,
    pub firmware: Vec<String>,
    pub serial: String,
    pub dpi: Option<DpiView>,
    pub wheel: Option<(bool, bool)>,
    pub smartshift: Option<(bool, u8)>,
    pub fn_inverted: Option<bool>,
    pub host: Option<(u8, u8)>,
    pub backlight: Option<BacklightView>,
    /// (keys that can be disabled, keys disabled) bitmasks.
    pub disable_keys: Option<(u8, u8)>,
    pub guard: GuardView,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatteryView {
    pub percent: Option<u8>,
    pub charging: bool,
    pub level: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuardEntry {
    pub at: u64,
    pub device: String,
    pub message: String,
    /// "info" | "restore" | "warn"
    pub tone: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub id: String,
    pub tone: String,
    pub title: String,
    pub body: String,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

enum Cmd {
    Apply(String),
    Refresh,
    SwitchHost(String, u8),
}

struct WorkerHandle {
    tx: Sender<Cmd>,
    alive: Arc<AtomicBool>,
}

pub type Emitter = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;

pub struct Engine {
    pub store: Store,
    pub config: RwLock<Config>,
    views: Mutex<BTreeMap<String, DeviceView>>,
    workers: Mutex<HashMap<String, WorkerHandle>>,
    log: Mutex<VecDeque<GuardEntry>>,
    pub notices: Mutex<Vec<Notice>>,
    pub art_files: Mutex<HashMap<String, PathBuf>>,
    catalog: Option<Catalog>,
    emitter: OnceLock<Emitter>,
    log_path: PathBuf,
}

impl Engine {
    pub fn new() -> Arc<Engine> {
        let store = Store::new(Store::default_dir());
        let mut notices = vec![];
        let config = match store.load() {
            Ok((c, LoadOutcome::Recovered { from, .. })) => {
                notices.push(Notice {
                    id: "recovered".into(),
                    tone: "warn".into(),
                    title: "Settings recovered".into(),
                    body: format!(
                        "Your settings file was damaged, so Keymodus restored the backup from {}. The damaged copy was kept next to it.",
                        from.file_stem().and_then(|s| s.to_str()).unwrap_or("earlier")
                    ),
                });
                c
            }
            Ok((c, LoadOutcome::Unrecoverable { .. })) => {
                notices.push(Notice {
                    id: "unrecoverable".into(),
                    tone: "warn".into(),
                    title: "Settings could not be read".into(),
                    body: "The settings file was damaged and no backup was readable. It was set aside, not deleted.".into(),
                });
                c
            }
            Ok((c, _)) => c,
            Err(e) => {
                notices.push(Notice { id: "io".into(), tone: "warn".into(), title: "Settings unavailable".into(), body: e.to_string() });
                Config::default()
            }
        };
        let log_dir = logi_core::paths::log_dir();
        let _ = std::fs::create_dir_all(&log_dir);
        Arc::new(Engine {
            store,
            config: RwLock::new(config),
            views: Mutex::new(BTreeMap::new()),
            workers: Mutex::new(HashMap::new()),
            log: Mutex::new(VecDeque::new()),
            notices: Mutex::new(notices),
            art_files: Mutex::new(HashMap::new()),
            catalog: Catalog::open(),
            emitter: OnceLock::new(),
            log_path: log_dir.join("keymodus.log"),
        })
    }

    pub fn set_emitter(&self, e: Emitter) {
        let _ = self.emitter.set(e);
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Some(e) = self.emitter.get() {
            e(event, payload);
        }
    }

    pub fn publish(&self) {
        self.emit("devices", serde_json::to_value(self.devices()).unwrap_or_default());
    }

    pub fn log(&self, device: &str, tone: &str, message: impl Into<String>) {
        let entry = GuardEntry { at: now_ms(), device: device.to_string(), message: message.into(), tone: tone.into() };
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.log_path) {
            let _ = writeln!(f, "{} [{}] {}: {}", entry.at, entry.tone, entry.device, entry.message);
        }
        {
            let mut log = self.log.lock().unwrap();
            log.push_front(entry.clone());
            log.truncate(200);
        }
        self.emit("guard", serde_json::to_value(entry).unwrap_or_default());
    }

    pub fn guard_log(&self) -> Vec<GuardEntry> {
        self.log.lock().unwrap().iter().cloned().collect()
    }

    /// Connected devices plus configured devices that are currently away.
    pub fn devices(&self) -> Vec<DeviceView> {
        let views = self.views.lock().unwrap();
        let cfg = self.config.read().unwrap();
        let mut out: Vec<DeviceView> = views.values().cloned().collect();
        for (id, dc) in &cfg.devices {
            if !views.contains_key(id) {
                out.push(offline_view(id, dc));
            }
        }
        out.sort_by_key(|d| (d.status == "offline", kind_rank(&d.kind), d.name.clone()));
        out
    }

    fn update_view(&self, id: &str, f: impl FnOnce(&mut DeviceView)) {
        let mut views = self.views.lock().unwrap();
        if let Some(v) = views.get_mut(id) {
            f(v);
        }
    }

    /// Apply a config mutation, persist it, and push it to the affected device.
    pub fn mutate(&self, device: Option<&str>, f: impl FnOnce(&mut Config)) -> Result<(), String> {
        let snapshot = {
            let mut cfg = self.config.write().unwrap();
            f(&mut cfg);
            cfg.clone()
        };
        self.store.save(&snapshot).map_err(|e| e.to_string())?;
        if let Some(id) = device {
            for w in self.workers.lock().unwrap().values() {
                let _ = w.tx.send(Cmd::Apply(id.to_string()));
            }
        }
        Ok(())
    }

    pub fn replace_config(&self, c: Config) {
        *self.config.write().unwrap() = c;
        for w in self.workers.lock().unwrap().values() {
            let _ = w.tx.send(Cmd::Refresh);
        }
    }

    /// Ask the device to move to another Easy-Switch channel.
    pub fn switch_host(&self, device: &str, host: u8) {
        for w in self.workers.lock().unwrap().values() {
            let _ = w.tx.send(Cmd::SwitchHost(device.to_string(), host));
        }
    }

    pub fn log_dir(&self) -> PathBuf {
        self.log_path.parent().map(PathBuf::from).unwrap_or_default()
    }

    pub fn reapply_all(&self) {
        for w in self.workers.lock().unwrap().values() {
            let _ = w.tx.send(Cmd::Refresh);
        }
    }

    // -----------------------------------------------------------------------
    // Supervisor
    // -----------------------------------------------------------------------

    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        thread::Builder::new().name("hid-supervisor".into()).spawn(move || me.supervise()).expect("spawn supervisor");
    }

    fn supervise(self: Arc<Self>) {
        let mut api = loop {
            match hidapi::HidApi::new() {
                Ok(a) => break a,
                Err(e) => {
                    self.log("Keymodus", "warn", format!("HID unavailable: {e}"));
                    thread::sleep(Duration::from_secs(5));
                }
            }
        };
        api.set_open_exclusive(false);
        // Log each failing interface once, not on every 2 s rescan.
        let mut open_failures: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let _ = api.refresh_devices();
            let mut seen: BTreeMap<String, (u16, String, bool)> = BTreeMap::new();
            for info in api.device_list() {
                if info.vendor_id() != hidpp::LOGITECH_VID {
                    continue;
                }
                let hidpp_bt = info.usage_page() == 0xFF43;
                let hidpp_usb = info.usage_page() == 0xFF00 && info.usage() == 0x0002;
                if !(hidpp_bt || hidpp_usb) {
                    continue;
                }
                let path = info.path().to_string_lossy().into_owned();
                let product = info.product_string().unwrap_or_default().to_string();
                let receiver = hidpp_usb && is_receiver(info.product_id(), &product);
                seen.entry(path).or_insert((info.product_id(), product, receiver));
            }

            let mut workers = self.workers.lock().unwrap();
            workers.retain(|_, w| w.alive.load(Ordering::Relaxed));
            for (path, (pid, product, receiver)) in seen {
                if workers.contains_key(&path) {
                    continue;
                }
                let Ok(cpath) = CString::new(path.clone()) else { continue };
                let dev = match api.open_path(&cpath) {
                    Ok(d) => d,
                    Err(e) => {
                        if open_failures.insert(path.clone()) {
                            let hint = if cfg!(target_os = "macos") {
                                " — allow Keymodus in System Settings › Privacy & Security › Input Monitoring"
                            } else if cfg!(target_os = "linux") {
                                " — install the Keymodus udev rule (see README)"
                            } else {
                                ""
                            };
                            self.log(&product, "warn", format!("Could not open device: {e}{hint}"));
                        }
                        continue;
                    }
                };
                let (tx, rx) = channel();
                let alive = Arc::new(AtomicBool::new(true));
                let me = self.clone();
                let alive2 = alive.clone();
                let transport = HidTransport(dev);
                let ctx = WorkerCtx { pid, product, receiver };
                let spawned = thread::Builder::new().name(format!("dev-{pid:04x}")).spawn(move || {
                    me.run_worker(transport, ctx, rx);
                    alive2.store(false, Ordering::Relaxed);
                });
                if spawned.is_ok() {
                    open_failures.remove(&path);
                    workers.insert(path, WorkerHandle { tx, alive });
                }
            }
            drop(workers);
            thread::sleep(Duration::from_secs(2));
        }
    }

    // -----------------------------------------------------------------------
    // Worker
    // -----------------------------------------------------------------------

    fn run_worker(self: &Arc<Self>, t: HidTransport, ctx: WorkerCtx, rx: Receiver<Cmd>) {
        let indices: Vec<u8> = if ctx.receiver { (1..=6).collect() } else { vec![hidpp::DIRECT] };
        let mut eps: Vec<Endpoint> = indices.into_iter().map(Endpoint::new).collect();
        let mut buf = [0u8; 64];
        let mut last_rx = Instant::now();

        loop {
            // Commands from the UI.
            while let Ok(cmd) = rx.try_recv() {
                for ep in eps.iter_mut().filter(|e| e.ready) {
                    match &cmd {
                        Cmd::Apply(id) if ep.id.as_deref() == Some(id.as_str()) => self.apply(&t, ep, "settings changed"),
                        Cmd::Apply(_) => {}
                        Cmd::Refresh => self.apply(&t, ep, "settings reloaded"),
                        Cmd::SwitchHost(id, host) if ep.id.as_deref() == Some(id.as_str()) => {
                            let mut d = ep.device(&t);
                            match f::set_host(&mut d, *host) {
                                Ok(()) => self.log(&ep.name, "info", format!("Switched to Easy-Switch channel {}", host + 1)),
                                Err(e) => self.log(&ep.name, "warn", format!("Could not switch channel: {e}")),
                            }
                        }
                        Cmd::SwitchHost(..) => {}
                    }
                }
            }

            // (Re)initialise endpoints that are not ready yet.
            let now = Instant::now();
            for ep in eps.iter_mut().filter(|e| !e.ready && now >= e.next_try) {
                if let Err(e) = self.init_endpoint(&t, &ctx, ep) {
                    ep.next_try = Instant::now() + if ep.id.is_some() { RETRY_ASLEEP } else { RETRY_EMPTY_SLOT };
                    if let Some(id) = &ep.id {
                        self.update_view(id, |v| v.status = "asleep".into());
                        self.publish();
                    }
                    if matches!(e, hidpp::Error::Transport(_)) {
                        break;
                    }
                }
            }

            // Read one report.
            match t.read(&mut buf, Duration::from_millis(40)) {
                Ok(0) => {}
                Ok(n) => {
                    let woke = last_rx.elapsed() > IDLE_WAKE;
                    last_rx = Instant::now();
                    if let Some(m) = Message::parse(&buf[..n]) {
                        self.route(&t, &mut eps, m);
                    }
                    if woke {
                        // First input after a quiet spell: the device may have slept and
                        // dropped its diversions. Check before the user notices.
                        for ep in eps.iter_mut().filter(|e| e.ready) {
                            self.verify(&t, ep, "woke from idle");
                        }
                    }
                }
                Err(hidpp::Error::Timeout) => {}
                Err(_) => break, // device gone
            }

            // Notifications that arrived mid-request.
            for ep in eps.iter_mut().filter(|e| e.ready) {
                for m in std::mem::take(&mut ep.backlog) {
                    self.handle_note(&t, ep, m);
                }
            }

            // Periodic guard + battery.
            for ep in eps.iter_mut().filter(|e| e.ready) {
                if ep.last_verify.elapsed() > VERIFY_EVERY {
                    self.verify(&t, ep, "scheduled check");
                }
                if ep.last_battery.elapsed() > BATTERY_EVERY {
                    ep.last_battery = Instant::now();
                    let mut d = ep.device(&t);
                    if let Ok(b) = f::battery(&mut d) {
                        let id = ep.id.clone().unwrap_or_default();
                        self.update_view(&id, |v| v.battery = Some(battery_view(b)));
                        self.publish();
                    }
                }
            }
        }

        // Interface disappeared: release held keys, mark devices offline.
        for ep in &mut eps {
            ep.release_all();
            if let Some(id) = &ep.id {
                self.views.lock().unwrap().remove(id);
                self.log(&ep.name, "info", "Disconnected");
            }
        }
        self.publish();
    }

    fn init_endpoint(&self, t: &HidTransport, ctx: &WorkerCtx, ep: &mut Endpoint) -> hidpp::Result<()> {
        let mut d = Device::new(t, ep.index);
        d.timeout = Duration::from_millis(700);
        d.ping()?;
        d.timeout = Duration::from_millis(1200);

        // Fill the feature cache from the feature table in one pass.
        let feats = f::list_features(&mut d).unwrap_or_default();
        let mut cache = HashMap::new();
        for (i, (id, _)) in feats.iter().enumerate() {
            cache.insert(*id, Some(i as u8));
        }
        let mut d = Device::new(t, ep.index).with_feature_cache(cache);
        d.timeout = Duration::from_millis(1200);
        let has = |id: u16| feats.iter().any(|(x, _)| *x == id);

        let info = if has(f::DEVICE_INFO) { f::device_info(&mut d).ok() } else { None };
        let pid = if ctx.receiver {
            info.as_ref()
                .and_then(|i| i.model_ids.iter().copied().find(|p| (0xB000..0xC000).contains(p)).or(i.model_ids.first().copied()))
                .unwrap_or(0)
        } else {
            ctx.pid
        };
        let id = format!("046d:{pid:04x}");
        let kind = if has(f::DEVICE_NAME) { f::device_kind(&mut d).unwrap_or(DeviceKind::Other) } else { DeviceKind::Other };
        let name = self
            .catalog
            .as_ref()
            .and_then(|c| c.display_name(pid))
            .or_else(|| has(f::DEVICE_NAME).then(|| f::device_name(&mut d).ok()).flatten())
            .unwrap_or_else(|| ctx.product.clone());

        let controls = if has(f::REPROG_CONTROLS_V4) { f::controls(&mut d).unwrap_or_default() } else { vec![] };
        let art = self.catalog.as_ref().and_then(|c| c.art(pid, info.as_ref().map(|i| i.ext_model).unwrap_or(0)));

        let view = DeviceView {
            id: id.clone(),
            name: name.clone(),
            kind: kind_name(kind).into(),
            connection: if ctx.receiver {
                "Receiver".into()
            } else if ctx.pid >= 0xB000 {
                "Bluetooth".into()
            } else {
                "USB".into()
            },
            status: "online".into(),
            battery: f::battery(&mut d).ok().map(battery_view),
            controls: control_views(&controls, art.as_ref(), kind),
            art: art.as_ref().map(|a| {
                self.art_files.lock().unwrap().insert(id.clone(), a.image.clone());
                art_view(&id, a)
            }),
            features: feats.iter().map(|(fid, _)| (format!("0x{fid:04X}"), f::feature_name(*fid).to_string())).collect(),
            firmware: info.as_ref().map(|i| i.firmware.clone()).unwrap_or_default(),
            serial: info.as_ref().map(|i| i.unit_id.clone()).unwrap_or_default(),
            dpi: has(f::ADJUSTABLE_DPI).then(|| f::dpi(&mut d).ok()).flatten().map(|x| DpiView { current: x.current, supported: x.supported }),
            wheel: has(f::HIRES_WHEEL).then(|| f::wheel_mode(&mut d).ok()).flatten().map(|w| (w.hires, w.inverted)),
            smartshift: has(f::SMART_SHIFT).then(|| f::smartshift(&mut d).ok()).flatten().map(|s| (s.ratchet, s.threshold)),
            fn_inverted: has(f::FN_INVERSION_MULTI_HOST).then(|| f::fn_inversion(&mut d).ok()).flatten(),
            host: has(f::CHANGE_HOST).then(|| f::host_info(&mut d).ok()).flatten(),
            backlight: has(f::BACKLIGHT2).then(|| f::backlight(&mut d).ok()).flatten().map(backlight_view),
            disable_keys: has(f::DISABLE_KEYS).then(|| f::disabled_keys(&mut d).ok()).flatten(),
            guard: GuardView::default(),
        };

        ep.id = Some(id.clone());
        ep.name = name.clone();
        ep.features = d.feature_cache().clone();
        ep.controls = controls;
        ep.reprog_index = d.feature_index(f::REPROG_CONTROLS_V4).ok().flatten();
        ep.wireless_index = d.feature_index(f::WIRELESS_STATUS).ok().flatten();
        ep.dpi_supported = view.dpi.as_ref().map(|x| x.supported.clone()).unwrap_or_default();
        ep.dpi_current = view.dpi.as_ref().map(|x| x.current).unwrap_or(0);
        ep.ready = true;
        ep.last_battery = Instant::now();

        // Remember the device so it stays listed while away. Additive only.
        {
            let mut cfg = self.config.write().unwrap();
            let entry = cfg.devices.entry(id.clone()).or_default();
            if entry.name != name || entry.kind != view.kind {
                entry.name = name.clone();
                entry.kind = view.kind.clone();
                let snapshot = cfg.clone();
                drop(cfg);
                let _ = self.store.save(&snapshot);
            }
        }

        self.views.lock().unwrap().insert(id.clone(), view);
        self.log(&name, "info", format!("Connected over {}", if ctx.receiver { "receiver" } else { "Bluetooth/USB" }));
        self.apply(t, ep, "connected");
        Ok(())
    }

    /// Push configuration to the device: diversions and pointer settings.
    fn apply(&self, t: &HidTransport, ep: &mut Endpoint, why: &str) {
        let Some(id) = ep.id.clone() else { return };
        let dc = self.config.read().unwrap().devices.get(&id).cloned().unwrap_or_default();
        let wanted = dc.diverted_controls();
        let mut d = ep.device(t);
        let mut failed = 0;

        for c in ep.controls.iter().filter(|c| c.divertable()) {
            let want = wanted.get(&c.cid).copied();
            let had = ep.diverted.contains_key(&c.cid);
            match want {
                Some(raw) => {
                    if f::set_divert(&mut d, c.cid, true, raw && c.raw_xy()).is_ok() {
                        ep.diverted.insert(c.cid, raw && c.raw_xy());
                    } else {
                        failed += 1;
                    }
                }
                None if had && f::set_divert(&mut d, c.cid, false, false).is_ok() => {
                    ep.diverted.remove(&c.cid);
                }
                None => {}
            }
        }

        let p = &dc.pointer;
        if let Some(dpi) = p.dpi {
            if dpi != ep.dpi_current && f::set_dpi(&mut d, dpi).is_ok() {
                ep.dpi_current = dpi;
            }
        }
        if p.hires_scroll.is_some() || p.invert_wheel.is_some() {
            if let Ok(cur) = f::wheel_mode(&mut d) {
                let _ = f::set_wheel_mode(&mut d, p.hires_scroll.unwrap_or(cur.hires), p.invert_wheel.unwrap_or(cur.inverted));
            }
        }
        if p.smartshift_ratchet.is_some() || p.smartshift_threshold.is_some() {
            if let Ok(cur) = f::smartshift(&mut d) {
                let s = f::SmartShift {
                    ratchet: p.smartshift_ratchet.unwrap_or(cur.ratchet),
                    threshold: p.smartshift_threshold.unwrap_or(cur.threshold),
                };
                let _ = f::set_smartshift(&mut d, s);
            }
        }
        if let Some(on) = dc.fn_inverted {
            let _ = f::set_fn_inversion(&mut d, on);
        }
        let mut backlight = None;
        if let Some(b) = &dc.backlight {
            if let Ok(cur) = f::backlight(&mut d) {
                // Only ask for modes the firmware advertises; otherwise keep its own.
                let mode = match b.mode.as_str() {
                    "manual" if cur.manual_supported => f::BacklightMode::Manual,
                    "auto" if cur.auto_supported => f::BacklightMode::Auto,
                    _ => cur.mode,
                };
                let wanted = (b.enabled, mode, if mode == f::BacklightMode::Manual { b.level } else { cur.level });
                if wanted != (cur.enabled, cur.mode, cur.level) {
                    let _ = f::set_backlight(&mut d, &cur, b.enabled, mode, b.level);
                }
                backlight = f::backlight(&mut d).ok().map(backlight_view);
            }
        }
        let mut disable_keys = None;
        if let Some(mask) = dc.disabled_keys {
            if let Ok((caps, cur)) = f::disabled_keys(&mut d) {
                if cur != mask & caps {
                    let _ = f::set_disabled_keys(&mut d, mask & caps);
                }
                disable_keys = Some((caps, mask & caps));
            }
        }

        let wheel = f::wheel_mode(&mut d).ok().map(|w| (w.hires, w.inverted));
        let ss = f::smartshift(&mut d).ok().map(|s| (s.ratchet, s.threshold));
        let fninv = dc.fn_inverted;
        ep.absorb(&mut d);
        ep.last_verify = Instant::now();

        let diverted = ep.diverted.len();
        let dpi_now = ep.dpi_current;
        self.update_view(&id, |v| {
            v.status = "online".into();
            v.guard.diverted = diverted;
            v.guard.verified_at = Some(now_ms());
            if let Some(dv) = v.dpi.as_mut() {
                dv.current = dpi_now;
            }
            if wheel.is_some() {
                v.wheel = wheel;
            }
            if ss.is_some() {
                v.smartshift = ss;
            }
            if fninv.is_some() {
                v.fn_inverted = fninv;
            }
            if backlight.is_some() {
                v.backlight = backlight;
            }
            if disable_keys.is_some() {
                v.disable_keys = disable_keys;
            }
        });
        if failed > 0 {
            ep.last_verify = Instant::now() - VERIFY_EVERY + Duration::from_secs(3);
            self.log(&ep.name, "warn", format!("{failed} binding(s) could not be applied yet ({why}); retrying shortly"));
        } else if diverted > 0 {
            self.log(&ep.name, "info", format!("{diverted} binding(s) active ({why})"));
        }
        self.publish();
    }

    /// Read back every diversion and restore any the device lost.
    fn verify(&self, t: &HidTransport, ep: &mut Endpoint, why: &str) {
        ep.last_verify = Instant::now();
        if ep.diverted.is_empty() {
            return;
        }
        let Some(id) = ep.id.clone() else { return };
        let mut d = ep.device(t);
        let mut restored = 0u32;
        let mut unreachable = false;
        let targets: Vec<(u16, bool)> = ep.diverted.iter().map(|(c, r)| (*c, *r)).collect();
        for (cid, raw) in targets {
            match f::get_reporting(&mut d, cid) {
                Ok(r) if r.diverted => {}
                Ok(_) => {
                    if f::set_divert(&mut d, cid, true, raw).is_ok() {
                        restored += 1;
                    }
                }
                Err(hidpp::Error::Timeout) => {
                    unreachable = true;
                    break;
                }
                Err(_) => {}
            }
        }
        ep.absorb(&mut d);
        if unreachable {
            self.update_view(&id, |v| v.status = "asleep".into());
            self.publish();
            return;
        }
        self.update_view(&id, |v| {
            v.status = "online".into();
            v.guard.verified_at = Some(now_ms());
            v.guard.restores += restored;
        });
        if restored > 0 {
            self.log(&ep.name, "restore", format!("Device had dropped {restored} binding(s) — restored ({why})"));
        }
        self.publish();
    }

    fn route(&self, t: &HidTransport, eps: &mut [Endpoint], m: Message) {
        // Receiver connection notice (HID++ 1.0, sub id 0x41): re-init that slot.
        if m.report_id == hidpp::REPORT_SHORT && m.feature_index == 0x41 {
            if let Some(ep) = eps.iter_mut().find(|e| e.index == m.device_index) {
                let link_up = m.u8_at(0) & 0x40 == 0;
                ep.release_all();
                ep.ready = false;
                ep.next_try = Instant::now() + if link_up { Duration::from_millis(300) } else { RETRY_ASLEEP };
                ep.diverted.clear();
                if let Some(id) = &ep.id {
                    self.update_view(id, |v| v.status = if link_up { "online".into() } else { "asleep".into() });
                }
            }
            return;
        }
        let Some(ep) = eps.iter_mut().find(|e| e.index == m.device_index && e.ready) else { return };
        self.handle_note(t, ep, m);
    }

    fn handle_note(&self, t: &HidTransport, ep: &mut Endpoint, m: Message) {
        if !m.is_notification() {
            return;
        }
        if Some(m.feature_index) == ep.reprog_index {
            if let Some(ev) = f::parse_control_event(&m) {
                self.on_control_event(t, ep, ev);
            }
        } else if Some(m.feature_index) == ep.wireless_index && f::is_reconfigure_request(&m) {
            self.log(&ep.name, "restore", "Device reconnected and asked to be reconfigured — bindings re-applied");
            ep.diverted.clear();
            self.apply(t, ep, "reconnected");
        }
    }

    fn on_control_event(&self, t: &HidTransport, ep: &mut Endpoint, ev: ControlEvent) {
        let Some(id) = ep.id.clone() else { return };
        match ev {
            ControlEvent::RawXY { dx, dy } => {
                if let Some(g) = ep.gesture.as_mut() {
                    g.dx += dx as i32;
                    g.dy += dy as i32;
                }
            }
            ControlEvent::Pressed(now) => {
                let pressed: Vec<u16> = now.iter().copied().filter(|c| !ep.pressed.contains(c)).collect();
                let released: Vec<u16> = ep.pressed.iter().copied().filter(|c| !now.contains(c)).collect();
                ep.pressed = now;
                for cid in pressed {
                    self.emit("press", serde_json::json!({ "device": id, "cid": cid, "down": true }));
                    let action = self.resolve(&id, cid);
                    match action {
                        Action::Gesture(g) => ep.gesture = Some(GestureTrack { cid, dx: 0, dy: 0, g }),
                        Action::CycleDpi => self.cycle_dpi(t, ep),
                        a if actions::is_holdable(&a) => {
                            actions::press(&a);
                            ep.held.insert(cid, a);
                        }
                        a => self.perform(&a),
                    }
                }
                for cid in released {
                    self.emit("press", serde_json::json!({ "device": id, "cid": cid, "down": false }));
                    if let Some(a) = ep.held.remove(&cid) {
                        actions::release(&a);
                    }
                    if ep.gesture.as_ref().is_some_and(|g| g.cid == cid) {
                        let g = ep.gesture.take().unwrap();
                        let threshold = self.config.read().unwrap().settings.gesture_threshold as i32;
                        let (ax, ay) = (g.dx.abs(), g.dy.abs());
                        let (dir, action) = if ax.max(ay) < threshold {
                            ("click", &g.g.click)
                        } else if ax > ay {
                            if g.dx > 0 {
                                ("right", &g.g.right)
                            } else {
                                ("left", &g.g.left)
                            }
                        } else if g.dy > 0 {
                            ("down", &g.g.down)
                        } else {
                            ("up", &g.g.up)
                        };
                        self.emit("gesture", serde_json::json!({ "device": id, "cid": cid, "dir": dir }));
                        let a = if action.is_default() { actions::native_action(cid) } else { (**action).clone() };
                        match a {
                            Action::CycleDpi => self.cycle_dpi(t, ep),
                            a => self.perform(&a),
                        }
                    }
                }
            }
        }
    }

    fn resolve(&self, device: &str, cid: u16) -> Action {
        let cfg = self.config.read().unwrap();
        let Some(dc) = cfg.devices.get(device) else { return actions::native_action(cid) };
        let bundle = if dc.app_bindings.is_empty() { None } else { platform::frontmost_app() };
        match dc.resolve(cid, bundle.as_deref()) {
            Action::Default => actions::native_action(cid),
            a => a,
        }
    }

    fn cycle_dpi(&self, t: &HidTransport, ep: &mut Endpoint) {
        let Some(id) = ep.id.clone() else { return };
        let presets = {
            let cfg = self.config.read().unwrap();
            cfg.devices.get(&id).map(|d| d.pointer.dpi_presets.clone()).unwrap_or_default()
        };
        let presets = if presets.len() >= 2 { presets } else { default_presets(&ep.dpi_supported) };
        if presets.is_empty() {
            return;
        }
        let next = presets.iter().copied().find(|&p| p > ep.dpi_current).unwrap_or(presets[0]);
        let mut d = ep.device(t);
        if f::set_dpi(&mut d, next).is_ok() {
            ep.dpi_current = next;
            self.update_view(&id, |v| {
                if let Some(x) = v.dpi.as_mut() {
                    x.current = next;
                }
            });
            self.emit("dpi", serde_json::json!({ "device": id, "dpi": next }));
            self.publish();
        }
        ep.absorb(&mut d);
    }

    /// Fire a one-shot action, expanding Smart Actions from the config.
    fn perform(&self, a: &Action) {
        match a {
            Action::SmartAction { id } => {
                let steps = self.config.read().unwrap().smart_actions.get(id).map(|s| s.steps.clone());
                if let Some(steps) = steps {
                    actions::run_sequence(steps);
                }
            }
            other => actions::tap(other),
        }
    }

    /// Run an action from the UI ("Try it").
    pub fn test_action(self: &Arc<Self>, a: &Action) {
        let a = a.clone();
        let me = self.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(350));
            match &a {
                Action::Gesture(g) => me.perform(&g.click),
                other => me.perform(other),
            }
        });
    }

    /// Run a Smart Action that isn't saved yet (editor preview).
    pub fn test_steps(&self, steps: Vec<logi_core::Step>) {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(350));
            actions::run_sequence(steps);
        });
    }

    /// Generic devices: called from the event tap thread.
    pub fn on_tap_event(&self, ev: TapEvent) -> bool {
        let (action, down) = match ev {
            TapEvent::MouseButton { button, down } => {
                if down {
                    self.emit("generic", serde_json::json!({ "kind": "mouse", "button": button }));
                }
                (self.config.read().unwrap().generic.mouse_buttons.get(&button).cloned(), down)
            }
            TapEvent::Key { code, down, modified } => {
                if modified {
                    return false;
                }
                (self.config.read().unwrap().generic.keys.get(code).cloned(), down)
            }
        };
        match action {
            None | Some(Action::Default) => false,
            Some(a) => {
                if actions::is_holdable(&a) {
                    if down {
                        actions::press(&a)
                    } else {
                        actions::release(&a)
                    }
                } else if down {
                    self.perform(&a);
                }
                true
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Worker-side state
// ---------------------------------------------------------------------------

struct WorkerCtx {
    pid: u16,
    product: String,
    receiver: bool,
}

struct GestureTrack {
    cid: u16,
    dx: i32,
    dy: i32,
    g: Gesture,
}

struct Endpoint {
    index: u8,
    id: Option<String>,
    name: String,
    ready: bool,
    next_try: Instant,
    features: HashMap<u16, Option<u8>>,
    controls: Vec<Control>,
    reprog_index: Option<u8>,
    wireless_index: Option<u8>,
    diverted: BTreeMap<u16, bool>,
    pressed: Vec<u16>,
    held: HashMap<u16, Action>,
    gesture: Option<GestureTrack>,
    dpi_supported: Vec<u16>,
    dpi_current: u16,
    last_verify: Instant,
    last_battery: Instant,
    backlog: Vec<Message>,
}

impl Endpoint {
    fn new(index: u8) -> Self {
        Endpoint {
            index,
            id: None,
            name: String::new(),
            ready: false,
            next_try: Instant::now(),
            features: HashMap::new(),
            controls: vec![],
            reprog_index: None,
            wireless_index: None,
            diverted: BTreeMap::new(),
            pressed: vec![],
            held: HashMap::new(),
            gesture: None,
            dpi_supported: vec![],
            dpi_current: 0,
            last_verify: Instant::now(),
            last_battery: Instant::now(),
            backlog: vec![],
        }
    }

    fn device<'t>(&self, t: &'t HidTransport) -> Device<'t, HidTransport> {
        let mut d = Device::new(t, self.index).with_feature_cache(self.features.clone());
        d.timeout = Duration::from_millis(800);
        d
    }

    /// Keep the feature cache, and park any notification that arrived while
    /// we were waiting on a reply so the worker loop still handles it.
    fn absorb(&mut self, d: &mut Device<HidTransport>) {
        self.features = d.feature_cache().clone();
        self.backlog.extend(d.take_notifications());
    }

    fn release_all(&mut self) {
        for (_, a) in self.held.drain() {
            actions::release(&a);
        }
        self.pressed.clear();
        self.gesture = None;
    }
}

pub struct HidTransport(hidapi::HidDevice);

impl Transport for HidTransport {
    fn write(&self, report: &[u8]) -> hidpp::Result<()> {
        self.0.write(report).map(|_| ()).map_err(|e| hidpp::Error::Transport(e.to_string()))
    }
    fn read(&self, buf: &mut [u8], timeout: Duration) -> hidpp::Result<usize> {
        match self.0.read_timeout(buf, timeout.as_millis().min(i32::MAX as u128) as i32) {
            Ok(0) => Err(hidpp::Error::Timeout),
            Ok(n) => Ok(n),
            Err(e) => Err(hidpp::Error::Transport(e.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_receiver(pid: u16, product: &str) -> bool {
    matches!(
        pid,
        0xC517 | 0xC51A | 0xC51B | 0xC52B | 0xC52F | 0xC531 | 0xC532 | 0xC534 | 0xC539 | 0xC53A | 0xC53D | 0xC53F | 0xC541 | 0xC545 | 0xC547 | 0xC548
    ) || product.contains("Receiver")
}

fn kind_name(k: DeviceKind) -> &'static str {
    match k {
        DeviceKind::Keyboard | DeviceKind::Numpad => "keyboard",
        DeviceKind::Mouse | DeviceKind::Trackball => "mouse",
        DeviceKind::Trackpad => "trackpad",
        DeviceKind::Presenter | DeviceKind::Remote => "presenter",
        DeviceKind::Receiver => "receiver",
        DeviceKind::Other => "device",
    }
}

fn kind_rank(k: &str) -> u8 {
    match k {
        "mouse" => 0,
        "keyboard" => 1,
        _ => 2,
    }
}

fn battery_view(b: f::Battery) -> BatteryView {
    BatteryView { percent: b.percent, charging: b.charging, level: b.level.into() }
}

fn backlight_view(b: f::Backlight) -> BacklightView {
    BacklightView {
        enabled: b.enabled,
        mode: match b.mode {
            f::BacklightMode::Basic => "basic",
            f::BacklightMode::Auto => "auto",
            f::BacklightMode::Manual => "manual",
        }
        .into(),
        level: b.level,
        levels: b.levels,
        auto_supported: b.auto_supported,
        manual_supported: b.manual_supported,
    }
}

fn art_view(id: &str, a: &DeviceArt) -> ArtView {
    ArtView { url: format!("devart://localhost/{id}"), width: a.width, height: a.height, hotspots: a.hotspots.clone() }
}

fn control_name(cid: u16) -> String {
    match cid {
        0x50 => "Left click".into(),
        0x51 => "Right click".into(),
        0x52 => "Middle button".into(),
        0x53 => "Back button".into(),
        0x56 => "Forward button".into(),
        0x5B => "Wheel tilt left".into(),
        0x5D => "Wheel tilt right".into(),
        0xC3 => "Gesture button".into(),
        0xC4 => "Wheel mode button".into(),
        0xD7 => "Thumb button".into(),
        0xFD => "Top button".into(),
        _ => format!("Control {cid}"),
    }
}

fn control_views(controls: &[Control], art: Option<&DeviceArt>, kind: DeviceKind) -> Vec<ControlView> {
    let keyboard = matches!(kind, DeviceKind::Keyboard | DeviceKind::Numpad);
    let mut out: Vec<ControlView> = controls
        .iter()
        .filter(|c| c.divertable() && !c.is_virtual() && c.cid != 0x50 && c.cid != 0x51)
        // Keyboards list many hidden, firmware-internal controls; only the
        // top-row keys are physical keys a person can press.
        .filter(|c| !keyboard || (c.is_fkey() && c.position > 0))
        .map(|c| {
            let fkey = (keyboard && c.is_fkey()).then_some(c.position);
            ControlView {
                cid: c.cid,
                name: art
                    .and_then(|a| a.hotspots.iter().find(|h| h.cid == c.cid))
                    .map(|h| h.name.clone())
                    .or_else(|| fkey.map(|n| format!("F{n} key")))
                    .unwrap_or_else(|| control_name(c.cid)),
                gesture: c.raw_xy(),
                mouse: c.is_mouse_button() || kind.is_pointer(),
                fkey,
            }
        })
        .collect();
    if keyboard {
        out.sort_by_key(|c| c.fkey);
    }
    // Order by artwork position when we have it (top row left to right).
    if let Some(a) = art {
        let pos = |cid: u16| a.hotspots.iter().position(|h| h.cid == cid).unwrap_or(usize::MAX);
        out.sort_by_key(|c| pos(c.cid));
    }
    out
}

fn default_presets(supported: &[u16]) -> Vec<u16> {
    if supported.is_empty() {
        return vec![];
    }
    let near = |t: u16| *supported.iter().min_by_key(|&&s| (s as i32 - t as i32).abs()).unwrap();
    let mut v = vec![near(800), near(1600), near(2400)];
    v.dedup();
    v
}

fn offline_view(id: &str, dc: &DeviceConfig) -> DeviceView {
    DeviceView {
        id: id.to_string(),
        name: if dc.name.is_empty() { id.to_string() } else { dc.name.clone() },
        kind: if dc.kind.is_empty() { "device".into() } else { dc.kind.clone() },
        connection: String::new(),
        status: "offline".into(),
        battery: None,
        controls: dc
            .bindings
            .keys()
            .chain(dc.app_bindings.values().flat_map(|m| m.keys()))
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|cid| ControlView { cid, name: control_name(cid), gesture: false, mouse: dc.kind == "mouse", fkey: None })
            .collect(),
        art: None,
        features: vec![],
        firmware: vec![],
        serial: String::new(),
        dpi: None,
        wheel: None,
        smartshift: None,
        fn_inverted: dc.fn_inverted,
        host: None,
        backlight: None,
        disable_keys: None,
        guard: GuardView::default(),
    }
}
