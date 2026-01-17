//! Tray helper - runs as user to show tray icon while main app runs as root
//!
//! Communication happens via a Unix socket or file-based IPC.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Get the socket path for IPC
pub fn socket_path() -> PathBuf {
    // Try XDG_RUNTIME_DIR first, fall back to /tmp with UID
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/tmp/razerlinux-{}", unsafe { libc::getuid() }));
    PathBuf::from(runtime_dir).join("razerlinux-tray.sock")
}

/// Commands sent between tray helper and main app
#[derive(Debug, Clone, PartialEq)]
pub enum IpcCommand {
    ShowWindow,
    Quit,
    Ping,
    Pong,
}

impl IpcCommand {
    pub fn to_string(&self) -> String {
        match self {
            IpcCommand::ShowWindow => "SHOW".to_string(),
            IpcCommand::Quit => "QUIT".to_string(),
            IpcCommand::Ping => "PING".to_string(),
            IpcCommand::Pong => "PONG".to_string(),
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "SHOW" => Some(IpcCommand::ShowWindow),
            "QUIT" => Some(IpcCommand::Quit),
            "PING" => Some(IpcCommand::Ping),
            "PONG" => Some(IpcCommand::Pong),
            _ => None,
        }
    }
}

/// Run the tray helper (called when --tray-helper flag is passed)
pub fn run_tray_helper() -> anyhow::Result<()> {
    use ksni::menu::{MenuItem, StandardItem};

    let socket_path = socket_path();
    
    // Remove old socket if exists
    let _ = fs::remove_file(&socket_path);
    
    // Create Unix socket listener
    let listener = UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;
    
    println!("Tray helper started, socket: {:?}", socket_path);
    
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    
    // Channel to send commands to main app
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<IpcCommand>();
    
    // Track connection to main app
    let main_stream: Arc<std::sync::Mutex<Option<UnixStream>>> = Arc::new(std::sync::Mutex::new(None));
    let main_stream_clone = main_stream.clone();
    
    // Create tray icon
    struct TrayHelper {
        cmd_tx: std::sync::mpsc::Sender<IpcCommand>,
    }
    
    impl ksni::Tray for TrayHelper {
        fn title(&self) -> String {
            "RazerLinux".to_string()
        }
        
        fn icon_name(&self) -> String {
            "input-mouse".to_string()
        }
        
        fn id(&self) -> String {
            "razerlinux".to_string()
        }
        
        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                MenuItem::Standard(StandardItem {
                    label: "Show RazerLinux".to_string(),
                    activate: Box::new(|this| {
                        let _ = this.cmd_tx.send(IpcCommand::ShowWindow);
                    }),
                    ..Default::default()
                }),
                MenuItem::Separator,
                MenuItem::Standard(StandardItem {
                    label: "Quit".to_string(),
                    activate: Box::new(|this| {
                        let _ = this.cmd_tx.send(IpcCommand::Quit);
                    }),
                    ..Default::default()
                }),
            ]
        }
    }
    
    let tray = TrayHelper { cmd_tx: cmd_tx.clone() };
    let service = ksni::TrayService::new(tray);
    service.spawn();
    
    println!("Tray icon created");
    
    // Thread to accept connections and read commands
    let running_accept = running.clone();
    thread::spawn(move || {
        while running_accept.load(Ordering::Relaxed) {
            // Accept new connections
            if let Ok((stream, _)) = listener.accept() {
                println!("Main app connected");
                stream.set_nonblocking(true).ok();
                *main_stream_clone.lock().unwrap() = Some(stream);
            }
            
            // Read from main app
            if let Some(ref mut stream) = *main_stream_clone.lock().unwrap() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Some(cmd) = IpcCommand::from_str(&line) {
                        if cmd == IpcCommand::Quit {
                            running_accept.store(false, Ordering::Relaxed);
                        }
                    }
                }
            }
            
            thread::sleep(Duration::from_millis(100));
        }
    });
    
    // Main loop - forward tray commands to main app
    while running_clone.load(Ordering::Relaxed) {
        // Check for tray menu commands
        if let Ok(cmd) = cmd_rx.try_recv() {
            if let Some(ref mut stream) = *main_stream.lock().unwrap() {
                let msg = format!("{}\n", cmd.to_string());
                let _ = stream.write_all(msg.as_bytes());
            }
            
            if cmd == IpcCommand::Quit {
                running_clone.store(false, Ordering::Relaxed);
            }
        }
        
        thread::sleep(Duration::from_millis(50));
    }
    
    // Cleanup
    let _ = fs::remove_file(&socket_path);
    println!("Tray helper exiting");
    
    Ok(())
}

/// Client to connect to tray helper from main app
pub struct TrayClient {
    stream: Option<UnixStream>,
}

impl TrayClient {
    /// Try to connect to the tray helper
    pub fn connect() -> Self {
        let socket_path = socket_path();
        let stream = UnixStream::connect(&socket_path).ok();
        if stream.is_some() {
            println!("Connected to tray helper");
        }
        Self { stream }
    }
    
    /// Check if connected to tray helper
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
    
    /// Check for commands from tray
    pub fn try_recv(&mut self) -> Option<IpcCommand> {
        if let Some(ref mut stream) = self.stream {
            stream.set_nonblocking(true).ok();
            let mut reader = BufReader::new(stream.try_clone().ok()?);
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) > 0 {
                return IpcCommand::from_str(&line);
            }
        }
        None
    }
    
    /// Send quit command to tray helper
    pub fn quit(&mut self) {
        if let Some(ref mut stream) = self.stream {
            let _ = stream.write_all(b"QUIT\n");
        }
    }
}
