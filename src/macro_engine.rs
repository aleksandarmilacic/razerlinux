//! Macro recording and playback engine
//! 
//! Handles capturing keystrokes during recording and executing macro sequences.

use crate::profile::{Macro, MacroAction, MacroActionType};
use anyhow::{Context, Result};
use evdev::{uinput::VirtualDeviceBuilder, AttributeSet, EventType, InputEvent, Key};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::thread;
use tracing::{info, warn};

/// Manages macro storage, recording, and playback
pub struct MacroManager {
    /// All saved macros (id -> Macro)
    macros: HashMap<u32, Macro>,
    /// Next available macro ID
    next_id: u32,
    /// Currently recording macro (if any)
    recording: Option<RecordingState>,
}

/// State during macro recording
struct RecordingState {
    /// Macro being built
    macro_data: Macro,
    /// Time of last action (for delay calculation)
    last_action_time: Instant,
}

impl MacroManager {
    /// Create a new empty macro manager
    pub fn new() -> Self {
        Self {
            macros: HashMap::new(),
            next_id: 1,
            recording: None,
        }
    }
    
    /// Get the next available macro ID
    pub fn get_next_id(&self) -> u32 {
        self.next_id
    }
    
    /// Start recording a new macro
    pub fn start_recording(&mut self, name: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        
        self.recording = Some(RecordingState {
            macro_data: Macro::new(id, name),
            last_action_time: Instant::now(),
        });
        
        info!("Started recording macro '{}' (id={})", name, id);
        id
    }
    
    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }
    
    /// Record a key press event
    pub fn record_key_press(&mut self, key_code: u16) {
        if let Some(ref mut state) = self.recording {
            // Add delay since last action (if significant)
            let elapsed = state.last_action_time.elapsed().as_millis() as u32;
            if elapsed > 10 && !state.macro_data.actions.is_empty() {
                state.macro_data.actions.push(MacroAction {
                    action_type: MacroActionType::Delay,
                    key_code: None,
                    delay_ms: Some(elapsed),
                });
            }
            
            // Add key press
            state.macro_data.actions.push(MacroAction {
                action_type: MacroActionType::KeyPress,
                key_code: Some(key_code),
                delay_ms: None,
            });
            
            state.last_action_time = Instant::now();
            info!("Recorded key press: {}", key_code);
        }
    }
    
    /// Record a key release event
    pub fn record_key_release(&mut self, key_code: u16) {
        if let Some(ref mut state) = self.recording {
            // Add delay since last action (if significant)
            let elapsed = state.last_action_time.elapsed().as_millis() as u32;
            if elapsed > 10 {
                state.macro_data.actions.push(MacroAction {
                    action_type: MacroActionType::Delay,
                    key_code: None,
                    delay_ms: Some(elapsed),
                });
            }
            
            // Add key release
            state.macro_data.actions.push(MacroAction {
                action_type: MacroActionType::KeyRelease,
                key_code: Some(key_code),
                delay_ms: None,
            });
            
            state.last_action_time = Instant::now();
            info!("Recorded key release: {}", key_code);
        }
    }
    
    /// Add a manual delay
    pub fn add_delay(&mut self, delay_ms: u32) {
        if let Some(ref mut state) = self.recording {
            state.macro_data.actions.push(MacroAction {
                action_type: MacroActionType::Delay,
                key_code: None,
                delay_ms: Some(delay_ms),
            });
            state.last_action_time = Instant::now();
            info!("Added delay: {}ms", delay_ms);
        }
    }
    
    /// Stop recording and save the macro
    pub fn stop_recording(&mut self) -> Option<Macro> {
        if let Some(state) = self.recording.take() {
            let macro_data = state.macro_data;
            info!("Stopped recording macro '{}' with {} actions", 
                  macro_data.name, macro_data.actions.len());
            
            // Save to our map
            self.macros.insert(macro_data.id, macro_data.clone());
            Some(macro_data)
        } else {
            None
        }
    }
    
    /// Cancel recording without saving
    pub fn cancel_recording(&mut self) {
        if self.recording.take().is_some() {
            info!("Recording cancelled");
        }
    }
    
    /// Get a macro by ID
    pub fn get_macro(&self, id: u32) -> Option<&Macro> {
        self.macros.get(&id)
    }
    
    /// Get all macros
    pub fn get_all_macros(&self) -> Vec<&Macro> {
        self.macros.values().collect()
    }
    
    /// Delete a macro by ID
    pub fn delete_macro(&mut self, id: u32) -> bool {
        self.macros.remove(&id).is_some()
    }
    
    /// Save a macro (update or insert)
    pub fn save_macro(&mut self, macro_data: Macro) {
        let id = macro_data.id;
        self.macros.insert(id, macro_data);
        if id >= self.next_id {
            self.next_id = id + 1;
        }
    }
    
    /// Update macro settings (name, repeat count)
    pub fn update_macro(&mut self, id: u32, name: &str, repeat_count: u32) -> bool {
        if let Some(m) = self.macros.get_mut(&id) {
            m.name = name.to_string();
            m.repeat_count = repeat_count;
            true
        } else {
            false
        }
    }
    
    /// Get current recording actions as display text
    pub fn get_recording_display_text(&self) -> String {
        if let Some(ref state) = self.recording {
            state.macro_data.to_display_text()
        } else {
            "Not recording".to_string()
        }
    }
    
    /// Get list of macros as display text
    pub fn get_macros_list_text(&self) -> String {
        if self.macros.is_empty() {
            return "No macros defined".to_string();
        }
        
        self.macros
            .values()
            .map(|m| format!("[{}] {} ({} actions)", m.id, m.name, m.actions.len()))
            .collect::<Vec<_>>()
            .join("\n")
    }
    
    /// Get available macros as a comma-separated list of "id:name" pairs for UI
    pub fn get_available_macros_string(&self) -> String {
        self.macros
            .values()
            .map(|m| format!("{}:{}", m.id, m.name))
            .collect::<Vec<_>>()
            .join(",")
    }
    
    /// Load macros from profile
    pub fn load_from_profile(&mut self, macros: Vec<Macro>) {
        self.macros.clear();
        for m in macros {
            if m.id >= self.next_id {
                self.next_id = m.id + 1;
            }
            self.macros.insert(m.id, m);
        }
        info!("Loaded {} macros from profile", self.macros.len());
    }
    
    /// Export macros for profile saving
    pub fn export_for_profile(&self) -> Vec<Macro> {
        self.macros.values().cloned().collect()
    }
}

impl Default for MacroManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a macro using a virtual input device
/// This runs in a separate thread to not block the UI
pub fn execute_macro(macro_data: &Macro) -> Result<()> {
    info!("Executing macro '{}' with {} actions", macro_data.name, macro_data.actions.len());
    
    if macro_data.actions.is_empty() {
        warn!("Macro has no actions");
        return Ok(());
    }
    
    // Build minimal key set needed
    let mut keys = AttributeSet::<Key>::new();
    for action in &macro_data.actions {
        if let Some(code) = action.key_code {
            keys.insert(Key::new(code));
        }
    }
    
    // Create virtual device for playback
    let mut vdev = VirtualDeviceBuilder::new()
        .context("Failed to create uinput builder")?
        .name("RazerLinux Macro Playback")
        .with_keys(&keys)
        .context("Failed to set key capabilities")?
        .build()
        .context("Failed to build uinput device")?;
    
    // Small delay for device to be recognized
    thread::sleep(Duration::from_millis(50));
    
    let repeat_count = if macro_data.repeat_count == 0 { 1 } else { macro_data.repeat_count };
    
    for _rep in 0..repeat_count {
        for action in &macro_data.actions {
            match action.action_type {
                MacroActionType::KeyPress => {
                    if let Some(code) = action.key_code {
                        emit_key(&mut vdev, code, 1)?;
                    }
                }
                MacroActionType::KeyRelease => {
                    if let Some(code) = action.key_code {
                        emit_key(&mut vdev, code, 0)?;
                    }
                }
                MacroActionType::Delay => {
                    if let Some(ms) = action.delay_ms {
                        thread::sleep(Duration::from_millis(ms as u64));
                    }
                }
                MacroActionType::MouseClick => {
                    if let Some(code) = action.key_code {
                        // Press and release
                        emit_key(&mut vdev, code, 1)?;
                        thread::sleep(Duration::from_millis(10));
                        emit_key(&mut vdev, code, 0)?;
                    }
                }
            }
        }
        
        // Delay between repeats
        if macro_data.repeat_count > 1 && macro_data.repeat_delay_ms > 0 {
            thread::sleep(Duration::from_millis(macro_data.repeat_delay_ms as u64));
        }
    }
    
    info!("Macro execution complete");
    Ok(())
}

/// Emit a key event
fn emit_key(vdev: &mut evdev::uinput::VirtualDevice, code: u16, value: i32) -> Result<()> {
    let events = [
        InputEvent::new(EventType::KEY, code, value),
        InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
    ];
    vdev.emit(&events).context("Failed to emit key event")?;
    Ok(())
}

/// Key code to human-readable name
pub fn key_name(code: u16) -> String {
    match code {
        1 => "ESC".to_string(),
        2..=11 => format!("{}", (code - 1) % 10), // 1-9, 0
        12 => "-".to_string(),
        13 => "=".to_string(),
        14 => "BACKSPACE".to_string(),
        15 => "TAB".to_string(),
        16 => "Q".to_string(),
        17 => "W".to_string(),
        18 => "E".to_string(),
        19 => "R".to_string(),
        20 => "T".to_string(),
        21 => "Y".to_string(),
        22 => "U".to_string(),
        23 => "I".to_string(),
        24 => "O".to_string(),
        25 => "P".to_string(),
        28 => "ENTER".to_string(),
        29 => "CTRL".to_string(),
        30 => "A".to_string(),
        31 => "S".to_string(),
        32 => "D".to_string(),
        33 => "F".to_string(),
        34 => "G".to_string(),
        35 => "H".to_string(),
        36 => "J".to_string(),
        37 => "K".to_string(),
        38 => "L".to_string(),
        42 => "SHIFT".to_string(),
        44 => "Z".to_string(),
        45 => "X".to_string(),
        46 => "C".to_string(),
        47 => "V".to_string(),
        48 => "B".to_string(),
        49 => "N".to_string(),
        50 => "M".to_string(),
        56 => "ALT".to_string(),
        57 => "SPACE".to_string(),
        58 => "CAPSLOCK".to_string(),
        59..=68 => format!("F{}", code - 58), // F1-F10
        87 => "F11".to_string(),
        88 => "F12".to_string(),
        183 => "F13".to_string(),
        184 => "F14".to_string(),
        272 => "LMB".to_string(),
        273 => "RMB".to_string(),
        274 => "MMB".to_string(),
        275 => "MB4".to_string(),
        276 => "MB5".to_string(),
        277 => "FORWARD".to_string(),
        278 => "BACK".to_string(),
        _ => format!("KEY_{}", code),
    }
}
