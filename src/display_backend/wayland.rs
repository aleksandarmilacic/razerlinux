//! Wayland Display Backend
//!
//! Provides scroll detection and overlay display for Wayland sessions.
//!
//! This module uses:
//! - AT-SPI for scroll detection (when available)
//! - Layer Shell protocol for overlay windows (via smithay-client-toolkit)

use super::{OverlayCommand, OverlayDisplay, ScrollDetector};
use anyhow::{Context, Result};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use tracing::{debug, info, warn};

// Re-export heuristic detector for fallback
pub use super::heuristic::HeuristicScrollDetector;

/// Wayland Scroll Detector
///
/// Uses AT-SPI for accurate scroll detection on Wayland.
/// Falls back to heuristic detection if AT-SPI is unavailable.
pub struct WaylandScrollDetector {
    /// Inner implementation (AT-SPI or heuristic)
    inner: Box<dyn ScrollDetector>,
}

impl WaylandScrollDetector {
    /// Create a new Wayland scroll detector
    pub fn new() -> Result<Self> {
        // Try AT-SPI first (best accuracy)
        #[cfg(feature = "atspi")]
        {
            match super::atspi::AtSpiScrollDetector::new() {
                Ok(detector) => {
                    info!("Wayland scroll detector using AT-SPI");
                    return Ok(Self {
                        inner: Box::new(detector),
                    });
                }
                Err(e) => {
                    warn!("AT-SPI unavailable on Wayland: {}", e);
                }
            }
        }

        // Fall back to heuristic detection
        info!("Wayland scroll detector using heuristic fallback");
        Ok(Self {
            inner: Box::new(HeuristicScrollDetector::new()),
        })
    }
}

impl ScrollDetector for WaylandScrollDetector {
    fn should_autoscroll(&self) -> bool {
        self.inner.should_autoscroll()
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        self.inner.cursor_position()
    }

    fn clear_cache(&self) {
        self.inner.clear_cache()
    }
}

/// Wayland Overlay Display
///
/// Shows the autoscroll indicator using the wlr-layer-shell protocol.
/// Falls back to a null overlay if layer-shell is not available.
pub struct WaylandOverlay {
    sender: Sender<OverlayCommand>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WaylandOverlay {
    /// Start the Wayland overlay system
    pub fn start() -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let thread = thread::spawn(move || {
            if let Err(e) = run_wayland_overlay_loop(rx) {
                // Layer shell might not be available on all compositors
                debug!("Wayland overlay unavailable: {:#}", e);
            }
        });

        Ok(Self {
            sender: tx,
            thread: Some(thread),
        })
    }
}

impl OverlayDisplay for WaylandOverlay {
    fn sender(&self) -> Sender<OverlayCommand> {
        self.sender.clone()
    }

    fn show(&self) {
        // Note: Uses (0,0) as fallback - callers should use sender directly with position
        let _ = self.sender.send(OverlayCommand::Show(0, 0));
    }

    fn hide(&self) {
        let _ = self.sender.send(OverlayCommand::Hide);
    }

    fn shutdown(&mut self) {
        let _ = self.sender.send(OverlayCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WaylandOverlay {
    fn drop(&mut self) {
        let _ = self.sender.send(OverlayCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Run the Wayland overlay event loop using smithay-client-toolkit
fn run_wayland_overlay_loop(rx: Receiver<OverlayCommand>) -> Result<()> {
    use smithay_client_toolkit::{
        compositor::{CompositorHandler, CompositorState},
        delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
        output::{OutputHandler, OutputState},
        registry::{ProvidesRegistryState, RegistryState},
        registry_handlers,
        shell::WaylandSurface,
        shell::wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        shm::{
            slot::{Buffer, SlotPool},
            Shm, ShmHandler,
        },
    };
    use wayland_client::{
        globals::registry_queue_init,
        protocol::{wl_output, wl_shm, wl_surface},
        Connection, QueueHandle,
    };

    const INDICATOR_SIZE: u32 = 32;

    struct OverlayState {
        registry_state: RegistryState,
        output_state: OutputState,
        compositor_state: CompositorState,
        shm: Shm,
        layer_shell: LayerShell,
        layer_surface: Option<LayerSurface>,
        pool: Option<SlotPool>,
        buffer: Option<Buffer>,
        visible: bool,
        running: bool,
        width: u32,
        height: u32,
        current_dx: f32,
        current_dy: f32,
        cursor_x: i32,
        cursor_y: i32,
    }

    impl CompositorHandler for OverlayState {
        fn scale_factor_changed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _new_factor: i32,
        ) {
        }

        fn transform_changed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _new_transform: wl_output::Transform,
        ) {
        }

        fn frame(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _time: u32,
        ) {
        }

        fn surface_enter(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _output: &wl_output::WlOutput,
        ) {
        }

        fn surface_leave(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _output: &wl_output::WlOutput,
        ) {
        }
    }

    impl OutputHandler for OverlayState {
        fn output_state(&mut self) -> &mut OutputState {
            &mut self.output_state
        }

        fn new_output(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
        }

        fn update_output(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
        }

        fn output_destroyed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
        }
    }

    impl LayerShellHandler for OverlayState {
        fn closed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _layer: &LayerSurface,
        ) {
            self.running = false;
        }

        fn configure(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _layer: &LayerSurface,
            configure: LayerSurfaceConfigure,
            _serial: u32,
        ) {
            self.width = configure.new_size.0.max(1);
            self.height = configure.new_size.1.max(1);

            // Drawing happens in the event loop, not here
        }
    }

    impl ShmHandler for OverlayState {
        fn shm_state(&mut self) -> &mut Shm {
            &mut self.shm
        }
    }

    impl ProvidesRegistryState for OverlayState {
        fn registry(&mut self) -> &mut RegistryState {
            &mut self.registry_state
        }

        registry_handlers![OutputState];
    }

    delegate_compositor!(OverlayState);
    delegate_output!(OverlayState);
    delegate_layer!(OverlayState);
    delegate_shm!(OverlayState);
    delegate_registry!(OverlayState);

    impl OverlayState {
        fn draw(&mut self, _qh: &QueueHandle<Self>) {
            let layer = match self.layer_surface {
                Some(ref l) => l,
                None => return,
            };

            let width = self.width;
            let height = self.height;
            let stride = width as i32 * 4;

            let pool = match self.pool.as_mut() {
                Some(p) => p,
                None => return,
            };

            let (buffer, canvas) = pool
                .create_buffer(
                    width as i32,
                    height as i32,
                    stride,
                    wl_shm::Format::Argb8888,
                )
                .expect("Failed to create buffer");

            // Draw the indicator
            draw_indicator_to_buffer(canvas, width, height, self.current_dx, self.current_dy);

            // Attach and commit
            buffer.attach_to(layer.wl_surface()).expect("Failed to attach buffer");
            layer.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
            layer.wl_surface().commit();

            self.buffer = Some(buffer);
        }
    }

    // Connect to Wayland
    let conn = Connection::connect_to_env().context("Failed to connect to Wayland")?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).context("Failed to init registry")?;
    let qh = event_queue.handle();

    // Get required globals
    let compositor_state =
        CompositorState::bind(&globals, &qh).context("Compositor not available")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("Layer shell not available")?;
    let shm = Shm::bind(&globals, &qh).context("Shm not available")?;
    let output_state = OutputState::new(&globals, &qh);
    let registry_state = RegistryState::new(&globals);

    // Create surface
    let surface = compositor_state.create_surface(&qh);

    // Create layer surface
    let layer_surface = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("razerlinux-autoscroll"),
        None, // All outputs
    );

    // Configure layer surface
    layer_surface.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer_surface.set_size(INDICATOR_SIZE, INDICATOR_SIZE);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer_surface.set_exclusive_zone(-1); // Don't reserve space
    layer_surface.wl_surface().commit();

    // Create buffer pool
    let pool = SlotPool::new(
        (INDICATOR_SIZE * INDICATOR_SIZE * 4) as usize,
        &shm,
    )
    .context("Failed to create buffer pool")?;

    let mut state = OverlayState {
        registry_state,
        output_state,
        compositor_state,
        shm,
        layer_shell,
        layer_surface: Some(layer_surface),
        pool: Some(pool),
        buffer: None,
        visible: false,
        running: true,
        width: INDICATOR_SIZE,
        height: INDICATOR_SIZE,
        current_dx: 0.0,
        current_dy: 0.0,
        cursor_x: 0,
        cursor_y: 0,
    };

    info!("Wayland layer-shell overlay initialized");

    // Event loop
    while state.running {
        // Check for commands (non-blocking)
        match rx.try_recv() {
            Ok(OverlayCommand::Show(cursor_x, cursor_y)) => {
                state.visible = true;
                state.current_dx = 0.0;
                state.current_dy = 0.0;
                state.cursor_x = cursor_x;
                state.cursor_y = cursor_y;
                
                // Position layer surface at cursor using margins
                // Center the indicator on the cursor
                let margin_left = (cursor_x - (INDICATOR_SIZE as i32 / 2)).max(0);
                let margin_top = (cursor_y - (INDICATOR_SIZE as i32 / 2)).max(0);
                
                if let Some(ref layer) = state.layer_surface {
                    layer.set_margin(margin_top, 0, 0, margin_left);
                    layer.wl_surface().commit();
                }
                
                state.draw(&qh);
                info!("Wayland overlay shown at ({}, {}) with margins (top={}, left={})", cursor_x, cursor_y, margin_top, margin_left);
            }
            Ok(OverlayCommand::Hide) => {
                state.visible = false;
                // Hide by making surface empty or moving off-screen
                if let Some(ref layer) = state.layer_surface {
                    layer.wl_surface().attach(None, 0, 0);
                    layer.wl_surface().commit();
                }
                info!("Wayland overlay hidden");
            }
            Ok(OverlayCommand::UpdateDirection(dx, dy)) => {
                if state.visible {
                    let dx_changed = (dx - state.current_dx).abs() > 0.2;
                    let dy_changed = (dy - state.current_dy).abs() > 0.2;
                    if dx_changed || dy_changed {
                        state.current_dx = dx;
                        state.current_dy = dy;
                        state.draw(&qh);
                    }
                }
            }
            Ok(OverlayCommand::Shutdown) => {
                info!("Wayland overlay shutting down");
                state.running = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                state.running = false;
            }
        }

        // Process Wayland events
        event_queue
            .blocking_dispatch(&mut state)
            .context("Wayland dispatch failed")?;
    }

    Ok(())
}

/// Draw the autoscroll indicator to a buffer
fn draw_indicator_to_buffer(canvas: &mut [u8], width: u32, height: u32, dx: f32, dy: f32) {
    let center_x = width as i32 / 2;
    let center_y = height as i32 / 2;

    // Clear with semi-transparent dark background
    for pixel in canvas.chunks_exact_mut(4) {
        pixel[0] = 0x33; // B
        pixel[1] = 0x33; // G
        pixel[2] = 0x33; // R
        pixel[3] = 0xDD; // A (semi-transparent)
    }

    // Helper to set a pixel
    let set_pixel = |canvas: &mut [u8], x: i32, y: i32, r: u8, g: u8, b: u8, a: u8| {
        if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
            let idx = ((y * width as i32 + x) * 4) as usize;
            if idx + 3 < canvas.len() {
                canvas[idx] = b;
                canvas[idx + 1] = g;
                canvas[idx + 2] = r;
                canvas[idx + 3] = a;
            }
        }
    };

    // Draw center dot (white)
    let dot_radius = 3i32;
    for dy_pix in -dot_radius..=dot_radius {
        for dx_pix in -dot_radius..=dot_radius {
            if dx_pix * dx_pix + dy_pix * dy_pix <= dot_radius * dot_radius {
                set_pixel(canvas, center_x + dx_pix, center_y + dy_pix, 0xFF, 0xFF, 0xFF, 0xFF);
            }
        }
    }

    // Arrow settings
    let arrow_offset = 9i32;
    let arrow_size = 4i32;

    let show_up = dy < -0.3;
    let show_down = dy > 0.3;
    let show_left = dx < -0.3;
    let show_right = dx > 0.3;
    let show_all = !show_up && !show_down && !show_left && !show_right;

    // Draw arrows (simple triangles)
    // Up arrow
    if show_up || show_all {
        let tip_y = center_y - arrow_offset - arrow_size;
        for row in 0..arrow_size {
            let y = tip_y + row;
            let half_width = row;
            for col in -half_width..=half_width {
                set_pixel(canvas, center_x + col, y, 0xFF, 0xFF, 0xFF, 0xFF);
            }
        }
    }

    // Down arrow
    if show_down || show_all {
        let tip_y = center_y + arrow_offset + arrow_size;
        for row in 0..arrow_size {
            let y = tip_y - row;
            let half_width = row;
            for col in -half_width..=half_width {
                set_pixel(canvas, center_x + col, y, 0xFF, 0xFF, 0xFF, 0xFF);
            }
        }
    }

    // Left arrow
    if show_left || show_all {
        let tip_x = center_x - arrow_offset - arrow_size;
        for col in 0..arrow_size {
            let x = tip_x + col;
            let half_height = col;
            for row in -half_height..=half_height {
                set_pixel(canvas, x, center_y + row, 0xFF, 0xFF, 0xFF, 0xFF);
            }
        }
    }

    // Right arrow
    if show_right || show_all {
        let tip_x = center_x + arrow_offset + arrow_size;
        for col in 0..arrow_size {
            let x = tip_x - col;
            let half_height = col;
            for row in -half_height..=half_height {
                set_pixel(canvas, x, center_y + row, 0xFF, 0xFF, 0xFF, 0xFF);
            }
        }
    }
}

#[cfg(feature = "atspi")]
pub use super::atspi::AtSpiScrollDetector;
