//! Autoscroll Visual Overlay
//!
//! Creates a small X11 overlay window at the cursor position to show
//! the autoscroll indicator (Windows-style).
//!
//! Windows autoscroll icon design:
//! - Small circle in the center (origin point marker)
//! - Four triangular arrows pointing up, down, left, right
//! - Semi-transparent dark background
//! - Clean, minimal design matching Windows style

use anyhow::{Context, Result};
use std::sync::mpsc::{self, Sender, Receiver};
use std::thread;
use tracing::{info, error};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

/// Size of the overlay indicator (pixels) - Windows typically uses ~24-32px
const INDICATOR_SIZE: u16 = 32;

/// Commands sent to the overlay thread
#[derive(Debug)]
pub enum OverlayCommand {
    /// Show overlay at cursor position
    Show,
    /// Hide overlay
    Hide,
    /// Update scroll direction (dx, dy normalized -1 to 1) - throttled updates only
    UpdateDirection(f32, f32),
    /// Shutdown the overlay thread
    Shutdown,
}

/// Handle to control the autoscroll overlay
pub struct AutoscrollOverlay {
    sender: Sender<OverlayCommand>,
    thread: Option<thread::JoinHandle<()>>,
}

impl AutoscrollOverlay {
    /// Start the overlay system
    pub fn start() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        
        let thread = thread::spawn(move || {
            if let Err(e) = run_overlay_loop(rx) {
                error!("Overlay thread error: {:#}", e);
            }
        });
        
        Ok(Self {
            sender: tx,
            thread: Some(thread),
        })
    }
    
    /// Get a sender to send commands to the overlay
    pub fn sender(&self) -> Sender<OverlayCommand> {
        self.sender.clone()
    }
    
    /// Show the overlay at current cursor position
    pub fn show(&self) {
        let _ = self.sender.send(OverlayCommand::Show);
    }
    
    /// Hide the overlay
    pub fn hide(&self) {
        let _ = self.sender.send(OverlayCommand::Hide);
    }
    
    /// Shutdown the overlay
    pub fn shutdown(mut self) {
        let _ = self.sender.send(OverlayCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AutoscrollOverlay {
    fn drop(&mut self) {
        let _ = self.sender.send(OverlayCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_overlay_loop(rx: Receiver<OverlayCommand>) -> Result<()> {
    // Connect to X11
    let (conn, screen_num) = x11rb::connect(None)
        .context("Failed to connect to X11 display")?;
    
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let depth = screen.root_depth;
    
    // Create the overlay window
    let win = conn.generate_id()?;
    
    // Window attributes for overlay:
    // - override_redirect: bypass window manager
    // - save_under: save what's behind the window  
    // - backing_store: always maintain window contents
    // - NO pointer/button events - window is "click through"
    // - background_pixmap NONE for transparency (we'll draw everything ourselves)
    let values = CreateWindowAux::new()
        .override_redirect(1)
        .save_under(1)
        .backing_store(BackingStore::ALWAYS)
        .background_pixmap(x11rb::NONE)  // No background for transparency
        .border_pixel(screen.white_pixel)
        .event_mask(EventMask::EXPOSURE);  // Only expose events, no input events
    
    conn.create_window(
        depth,
        win,
        root,
        0, 0,  // Position (will be updated when shown)
        INDICATOR_SIZE,
        INDICATOR_SIZE,
        0,  // No border - reduces visual interference
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &values,
    )?;
    
    // Make the window click-through using XShape extension
    // Set input shape to empty rectangle - all clicks pass through
    use x11rb::protocol::shape::{self, SK};
    let empty_region: &[Rectangle] = &[];
    shape::rectangles(
        &conn,
        shape::SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        win,
        0, 0,
        empty_region,
    )?;
    
    // Also set bounding shape to empty initially - we'll update it when drawing
    // This makes the window transparent except where we draw
    shape::rectangles(
        &conn,
        shape::SO::SET,
        SK::BOUNDING,
        ClipOrdering::UNSORTED,
        win,
        0, 0,
        empty_region,
    )?;
    
    // Create a graphics context for drawing
    let gc = conn.generate_id()?;
    let gc_values = CreateGCAux::new()
        .foreground(screen.white_pixel)
        .background(screen.black_pixel)
        .line_width(2);
    conn.create_gc(gc, win, &gc_values)?;
    
    // Create graphics context for filled shapes
    let gc_fill = conn.generate_id()?;
    let gc_fill_values = CreateGCAux::new()
        .foreground(0x00AA00)  // Green color
        .background(screen.black_pixel);
    conn.create_gc(gc_fill, win, &gc_fill_values)?;
    
    conn.flush()?;
    
    info!("Overlay window created");
    
    let mut visible = false;
    let mut current_dx: f32 = 0.0;
    let mut current_dy: f32 = 0.0;
    
    loop {
        // Check for commands with timeout
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(OverlayCommand::Show) => {
                // Get cursor position
                if let Ok(reply) = conn.query_pointer(root) {
                    if let Ok(pointer) = reply.reply() {
                        let x = pointer.root_x as i16 - (INDICATOR_SIZE as i16 / 2);
                        let y = pointer.root_y as i16 - (INDICATOR_SIZE as i16 / 2);
                        
                        // Move and show window
                        conn.configure_window(
                            win,
                            &ConfigureWindowAux::new().x(x as i32).y(y as i32),
                        )?;
                        conn.map_window(win)?;
                        conn.flush()?;
                        
                        visible = true;
                        current_dx = 0.0;
                        current_dy = 0.0;
                        
                        // Draw initial indicator (no direction)
                        draw_indicator(&conn, win, gc, gc_fill, 0.0, 0.0)?;
                        
                        info!("Overlay shown at ({}, {})", x, y);
                    }
                }
            }
            Ok(OverlayCommand::Hide) => {
                if visible {
                    conn.unmap_window(win)?;
                    conn.flush()?;
                    visible = false;
                    current_dx = 0.0;
                    current_dy = 0.0;
                    info!("Overlay hidden");
                }
            }
            Ok(OverlayCommand::UpdateDirection(dx, dy)) => {
                if visible {
                    // Only redraw if direction changed significantly
                    let dx_changed = (dx - current_dx).abs() > 0.2;
                    let dy_changed = (dy - current_dy).abs() > 0.2;
                    if dx_changed || dy_changed {
                        current_dx = dx;
                        current_dy = dy;
                        draw_indicator(&conn, win, gc, gc_fill, dx, dy)?;
                    }
                }
            }
            Ok(OverlayCommand::Shutdown) => {
                info!("Overlay shutting down");
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Process X11 events if any
                while let Some(event) = conn.poll_for_event()? {
                    match event {
                        x11rb::protocol::Event::Expose(_) => {
                            if visible {
                                draw_indicator(&conn, win, gc, gc_fill, current_dx, current_dy)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("Overlay channel disconnected");
                break;
            }
        }
    }
    
    // Cleanup
    conn.destroy_window(win)?;
    conn.free_gc(gc)?;
    conn.free_gc(gc_fill)?;
    conn.flush()?;
    
    Ok(())
}

fn draw_indicator<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    _gc_fill: Gcontext,
    dx: f32,
    dy: f32,
) -> Result<()> {
    use x11rb::protocol::shape::{self, SK};
    
    let size = INDICATOR_SIZE as i16;
    let center = size / 2;
    
    // Set bounding shape to define the visible (non-transparent) region
    // We create a circular region around the center for the icon
    let icon_radius = 14i16;  // Radius of the visible icon area
    let bounding_rects = [
        // Create a rough circle using rectangles (octagon approximation)
        Rectangle { x: center - icon_radius + 4, y: center - icon_radius, width: (icon_radius * 2 - 8) as u16, height: (icon_radius * 2) as u16 },
        Rectangle { x: center - icon_radius + 2, y: center - icon_radius + 2, width: (icon_radius * 2 - 4) as u16, height: (icon_radius * 2 - 4) as u16 },
        Rectangle { x: center - icon_radius, y: center - icon_radius + 4, width: (icon_radius * 2) as u16, height: (icon_radius * 2 - 8) as u16 },
    ];
    shape::rectangles(
        conn,
        shape::SO::SET,
        SK::BOUNDING,
        ClipOrdering::UNSORTED,
        win,
        0, 0,
        &bounding_rects,
    )?;
    
    // Clear and fill with a semi-dark background for the icon area
    // First fill the entire window with the background color
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x333333))?;
    conn.poly_fill_rectangle(win, gc, &[
        Rectangle { x: 0, y: 0, width: INDICATOR_SIZE, height: INDICATOR_SIZE },
    ])?;
    
    // Reset to white for drawing the icon
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0xFFFFFF))?;
    
    // Windows-style autoscroll icon:
    // - Small filled circle in the center (origin point)
    // - Four directional arrows around it
    
    // Draw center dot (origin point) - small filled circle
    let dot_radius = 3i16;
    conn.poly_fill_arc(win, gc, &[Arc {
        x: center - dot_radius,
        y: center - dot_radius,
        width: (dot_radius * 2) as u16,
        height: (dot_radius * 2) as u16,
        angle1: 0,
        angle2: 360 * 64,
    }])?;
    
    // Arrow positioning - closer to center like Windows
    let arrow_offset = 9i16;  // Distance from center to arrow
    let arrow_size = 4i16;    // Size of arrow triangles
    
    // Calculate which arrows to show based on scroll direction
    let show_up = dy < -0.3;
    let show_down = dy > 0.3;
    let show_left = dx < -0.3;
    let show_right = dx > 0.3;
    let show_all = !show_up && !show_down && !show_left && !show_right;
    
    // Up arrow (filled triangle pointing up)
    if show_up || show_all {
        let tip_y = center - arrow_offset - arrow_size;
        let base_y = center - arrow_offset + 1;
        draw_filled_arrow_up(conn, win, gc, center, tip_y, base_y, arrow_size)?;
    }
    
    // Down arrow (filled triangle pointing down)
    if show_down || show_all {
        let tip_y = center + arrow_offset + arrow_size;
        let base_y = center + arrow_offset - 1;
        draw_filled_arrow_down(conn, win, gc, center, tip_y, base_y, arrow_size)?;
    }
    
    // Left arrow (filled triangle pointing left)
    if show_left || show_all {
        let tip_x = center - arrow_offset - arrow_size;
        let base_x = center - arrow_offset + 1;
        draw_filled_arrow_left(conn, win, gc, tip_x, center, base_x, arrow_size)?;
    }
    
    // Right arrow (filled triangle pointing right)
    if show_right || show_all {
        let tip_x = center + arrow_offset + arrow_size;
        let base_x = center + arrow_offset - 1;
        draw_filled_arrow_right(conn, win, gc, tip_x, center, base_x, arrow_size)?;
    }
    
    conn.flush()?;
    Ok(())
}

fn draw_circle<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    cx: i16,
    cy: i16,
    radius: i16,
) -> Result<()> {
    conn.poly_arc(win, gc, &[Arc {
        x: cx - radius,
        y: cy - radius,
        width: (radius * 2) as u16,
        height: (radius * 2) as u16,
        angle1: 0,
        angle2: 360 * 64,
    }])?;
    Ok(())
}

// Windows-style filled arrow functions
// These draw solid triangular arrows pointing in each direction

fn draw_filled_arrow_up<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    x: i16,
    tip_y: i16,
    base_y: i16,
    half_width: i16,
) -> Result<()> {
    // Filled triangle pointing up
    let points = [
        Point { x, y: tip_y },                    // Top tip
        Point { x: x - half_width, y: base_y },   // Bottom left
        Point { x: x + half_width, y: base_y },   // Bottom right
    ];
    conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    Ok(())
}

fn draw_filled_arrow_down<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    x: i16,
    tip_y: i16,
    base_y: i16,
    half_width: i16,
) -> Result<()> {
    // Filled triangle pointing down
    let points = [
        Point { x, y: tip_y },                    // Bottom tip
        Point { x: x - half_width, y: base_y },   // Top left
        Point { x: x + half_width, y: base_y },   // Top right
    ];
    conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    Ok(())
}

fn draw_filled_arrow_left<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    tip_x: i16,
    y: i16,
    base_x: i16,
    half_height: i16,
) -> Result<()> {
    // Filled triangle pointing left
    let points = [
        Point { x: tip_x, y },                    // Left tip
        Point { x: base_x, y: y - half_height },  // Top right
        Point { x: base_x, y: y + half_height },  // Bottom right
    ];
    conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    Ok(())
}

fn draw_filled_arrow_right<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    tip_x: i16,
    y: i16,
    base_x: i16,
    half_height: i16,
) -> Result<()> {
    // Filled triangle pointing right
    let points = [
        Point { x: tip_x, y },                    // Right tip
        Point { x: base_x, y: y - half_height },  // Top left
        Point { x: base_x, y: y + half_height },  // Bottom left
    ];
    conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    Ok(())
}
