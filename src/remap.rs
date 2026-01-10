//! Software button remapping (evdev grab + uinput virtual device)

use anyhow::{Context, Result};
use evdev::{AttributeSet, Device, EventType, InputEvent, InputEventKind, Key, uinput::VirtualDeviceBuilder};
use std::collections::BTreeMap;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Default)]
pub struct RemapConfig {
    pub source_device: Option<String>,
    pub mappings: BTreeMap<u16, MappingTarget>,
}

#[derive(Debug, Clone, Default)]
pub struct MappingTarget {
    pub base: u16,
    pub mods: Modifiers,
}

#[derive(Debug, Clone, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub fn to_key_codes(&self) -> impl Iterator<Item = u16> + '_ {
        let mut codes: [u16; 4] = [0; 4];
        let mut len = 0;
        if self.ctrl {
            codes[len] = Key::KEY_LEFTCTRL.0;
            len += 1;
        }
        if self.alt {
            codes[len] = Key::KEY_LEFTALT.0;
            len += 1;
        }
        if self.shift {
            codes[len] = Key::KEY_LEFTSHIFT.0;
            len += 1;
        }
        if self.meta {
            codes[len] = Key::KEY_LEFTMETA.0;
            len += 1;
        }

        codes.into_iter().take(len)
    }
}

pub struct Remapper {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Remapper {
    pub fn start(config: RemapConfig) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let join = thread::spawn(move || {
            if let Err(e) = run_remapper_loop(stop_thread, config) {
                warn!("remapper stopped: {e:#}");
            }
        });

        Ok(Self {
            stop,
            join: Some(join),
        })
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Remapper {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn run_remapper_loop(stop: Arc<AtomicBool>, config: RemapConfig) -> Result<()> {
    let source_path = match select_source_device(&config.source_device) {
        Some(p) => p,
        None => anyhow::bail!("No suitable /dev/input/event* device found for the mouse"),
    };

    info!("Starting remapper on {source_path:?}");

    let mut dev = Device::open(&source_path)
        .with_context(|| format!("Failed to open evdev device: {source_path:?}"))?;

    set_nonblocking(&dev).context("Failed to set evdev device non-blocking")?;

    // Grab the device so the original events don't reach the system.
    // (The virtual device will emit the modified events instead.)
    dev.grab().context("Failed to grab evdev device")?;

    let mut vbuilder = VirtualDeviceBuilder::new().context("Failed to create uinput builder")?;
    vbuilder = vbuilder.name(&"RazerLinux Virtual Device");

    // Extend key capabilities to include targets + modifiers so the virtual device can emit them.
    let mut keys: AttributeSet<Key> = AttributeSet::new();
    if let Some(src_keys) = dev.supported_keys() {
        for k in src_keys.iter() {
            keys.insert(k);
        }
    }
    for target in config.mappings.values() {
        keys.insert(Key::new(target.base));
        for m in target.mods.to_key_codes() {
            keys.insert(Key::new(m));
        }
    }

    vbuilder = vbuilder
        .with_keys(&keys)
        .context("Failed to set key capabilities")?;
    if let Some(rel) = dev.supported_relative_axes() {
        vbuilder = vbuilder
            .with_relative_axes(&rel)
            .context("Failed to set relative axis capabilities")?;
    }
    // Most mice only need relative axes. Absolute axes require per-axis UinputAbsSetup.

    let mut vdev = vbuilder.build().context("Failed to build uinput device")?;

    while !stop.load(Ordering::Relaxed) {
        match dev.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let Some(mapped_events) = remap_events(&config.mappings, ev) {
                        if let Err(e) = vdev.emit(&mapped_events) {
                            warn!("uinput emit failed: {e}");
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e).context("Failed to read events from evdev device"),
        }
    }

    // Best-effort ungrab. (Dropping the device should also release the grab.)
    let _ = dev.ungrab();
    Ok(())
}

fn remap_events(
    mappings: &BTreeMap<u16, MappingTarget>,
    ev: InputEvent,
) -> Option<Vec<InputEvent>> {
    match ev.kind() {
        InputEventKind::Key(key) => {
            let src_code: u16 = key.code();
            let value = ev.value();
            if let Some(target) = mappings.get(&src_code) {
                let mut out: Vec<InputEvent> = Vec::new();

                match value {
                    1 => {
                        // Press: press modifiers first, then base key
                        for m in target.mods.to_key_codes() {
                            out.push(InputEvent::new(EventType::KEY, m, 1));
                        }
                        out.push(InputEvent::new(EventType::KEY, target.base, 1));
                    }
                    0 => {
                        // Release: release base, then modifiers
                        out.push(InputEvent::new(EventType::KEY, target.base, 0));
                        for m in target.mods.to_key_codes() {
                            out.push(InputEvent::new(EventType::KEY, m, 0));
                        }
                    }
                    2 => {
                        // Repeat: repeat base only
                        out.push(InputEvent::new(EventType::KEY, target.base, 2));
                    }
                    _ => {
                        out.push(ev);
                    }
                }

                Some(out)
            } else {
                Some(vec![ev])
            }
        }
        _ => Some(vec![ev]),
    }
}

pub fn capture_next_key_code(timeout: Duration, preferred_device: Option<&str>) -> Result<u16> {
    let source_path = match select_source_device(&preferred_device.map(|s| s.to_string())) {
        Some(p) => p,
        None => anyhow::bail!("No suitable /dev/input/event* device found"),
    };

    let mut dev = Device::open(&source_path)
        .with_context(|| format!("Failed to open evdev device: {source_path:?}"))?;
    set_nonblocking(&dev).context("Failed to set evdev device non-blocking")?;

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match dev.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let InputEventKind::Key(key) = ev.kind() {
                        // value 1 == key down
                        if ev.value() == 1 {
                            return Ok(key.code());
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e).context("Failed reading from evdev device"),
        }
    }

    anyhow::bail!("Timed out waiting for a button press")
}

fn select_source_device(preferred_device: &Option<String>) -> Option<PathBuf> {
    if let Some(p) = preferred_device {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // Heuristic: prefer devices whose name mentions Razer/Naga and expose REL_X/REL_Y.
    // Fall back to the first device that has relative axes.
    let mut fallback: Option<PathBuf> = None;

    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or_default().to_string();
        let has_rel = dev
            .supported_relative_axes()
            .map(|r| r.iter().next().is_some())
            .unwrap_or(false);
        if !has_rel {
            continue;
        }

        if fallback.is_none() {
            fallback = Some(path.clone());
        }

        let name_lower = name.to_ascii_lowercase();
        if name_lower.contains("razer") || name_lower.contains("naga") {
            return Some(path);
        }
    }

    fallback
}

fn set_nonblocking(dev: &Device) -> Result<()> {
    let raw_fd = dev.as_raw_fd();

    // Preserve existing flags; just OR in O_NONBLOCK.
    let current = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
    if current < 0 {
        return Err(std::io::Error::last_os_error()).context("fcntl(F_GETFL) failed");
    }

    let rc = unsafe { libc::fcntl(raw_fd, libc::F_SETFL, current | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error()).context("fcntl(F_SETFL, O_NONBLOCK) failed");
    }
    Ok(())
}
