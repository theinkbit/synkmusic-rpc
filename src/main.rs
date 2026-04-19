use clap::Parser;
use futures_util::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use serde_repr::Serialize_repr;
use std::io::{Read, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use uuid::Uuid;

const APP_ID: &str = "1489549668810493993";
const DEFAULT_PORT: u16 = 19836;
const SYNKMUSIC_URL: &str = "https://synkmusic.com";
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

#[cfg(unix)]
const KNOWN_SUBPATHS: &[&str] = &[
    "",
    "app/com.discordapp.Discord",
    "app/dev.vencord.Vesktop",
    "app/dev.equibop.equibop",
    "snap.discord",
    "snap.discord-canary",
];

#[derive(Parser)]
#[command(name = "synkmusic-rpc", about = "SYNK Music Discord RPC")]
struct Args {
    #[arg(short, long, default_value_t = DEFAULT_PORT, help = "Port to listen on")]
    port: u16,

    #[cfg(unix)]
    #[arg(long, help = "Path to Discord IPC socket (overrides auto-discovery)")]
    ipc_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum RpcMessage {
    #[serde(rename = "update")]
    Update {
        track: String,
        artist: String,
        #[serde(default)]
        album: Option<String>,
        #[serde(default)]
        elapsed: f64,
        #[serde(default)]
        duration: f64,
        #[serde(default)]
        cover_url: Option<String>,
        #[serde(default)]
        track_url: Option<String>,
        #[serde(default, rename = "playerStatus")]
        player_status: Option<String>,
    },
    #[serde(rename = "clear")]
    Clear,
}

#[cfg(unix)]
struct DiscordIpc {
    socket: std::os::unix::net::UnixStream,
}

#[cfg(windows)]
struct DiscordIpc {
    socket: std::fs::File,
}

#[cfg(unix)]
impl DiscordIpc {
    fn connect(ipc_path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        use std::os::unix::net::UnixStream;
        let socket = if let Some(path) = ipc_path {
            UnixStream::connect(path)?
        } else {
            Self::discover_socket()?
        };
        let mut client = Self { socket };
        client.handshake()?;
        Ok(client)
    }

    fn discover_socket() -> Result<std::os::unix::net::UnixStream, Box<dyn std::error::Error>> {
        use std::os::unix::net::UnixStream;
        use std::path::Path;

        let base_dirs = Self::get_base_dirs();

        for base in &base_dirs {
            for subpath in KNOWN_SUBPATHS {
                for i in 0..10 {
                    let path = Path::new(base).join(subpath).join(format!("discord-ipc-{i}"));
                    if let Ok(stream) = UnixStream::connect(&path) {
                        println!("[synkmusic-rpc] Found IPC socket: {}", path.display());
                        return Ok(stream);
                    }
                }
            }

            let flatpak_dir = Path::new(base).join(".flatpak");
            if let Ok(entries) = std::fs::read_dir(&flatpak_dir) {
                for entry in entries.flatten() {
                    let xdg_run = entry.path().join("xdg-run");
                    for i in 0..10 {
                        let path = xdg_run.join(format!("discord-ipc-{i}"));
                        if let Ok(stream) = UnixStream::connect(&path) {
                            println!("[synkmusic-rpc] Found IPC socket: {}", path.display());
                            return Ok(stream);
                        }
                    }
                }
            }

            let app_dir = Path::new(base).join("app");
            if let Ok(entries) = std::fs::read_dir(&app_dir) {
                for entry in entries.flatten() {
                    if !entry.path().is_dir() {
                        continue;
                    }
                    for i in 0..10 {
                        let path = entry.path().join(format!("discord-ipc-{i}"));
                        if let Ok(stream) = UnixStream::connect(&path) {
                            println!("[synkmusic-rpc] Found IPC socket: {}", path.display());
                            return Ok(stream);
                        }
                    }
                }
            }
        }

        Err("Could not find any Discord IPC socket".into())
    }

    fn get_base_dirs() -> Vec<std::path::PathBuf> {
        use std::path::PathBuf;
        let mut dirs = Vec::new();
        for key in &["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
            if let Ok(val) = std::env::var(key) {
                let path = PathBuf::from(val);
                if !dirs.contains(&path) {
                    dirs.push(path);
                }
            }
        }
        let tmp = PathBuf::from("/tmp");
        if !dirs.contains(&tmp) {
            dirs.push(tmp);
        }
        dirs
    }

    fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.send(2, &serde_json::json!({}));
        let _ = self.socket.flush();
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
}

#[cfg(windows)]
impl DiscordIpc {
    fn connect(_ipc_path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        use std::path::PathBuf;

        for i in 0..10 {
            let path = PathBuf::from(format!(r"\\.\pipe\discord-ipc-{}", i));
            if let Ok(handle) = OpenOptions::new().read(true).write(true).access_mode(0x3).open(&path) {
                println!("[synkmusic-rpc] Found IPC pipe: {}", path.display());
                let mut client = Self { socket: handle };
                client.handshake()?;
                return Ok(client);
            }
        }

        Err("Could not find any Discord IPC pipe".into())
    }

    fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.send(2, &serde_json::json!({}));
        let _ = self.socket.flush();
        Ok(())
    }
}

impl DiscordIpc {
    fn handshake(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let payload = serde_json::json!({
            "v": 1,
            "client_id": APP_ID
        });
        self.send(0, &payload)?;
        self.recv()?;
        Ok(())
    }

    fn send(
        &mut self,
        opcode: u32,
        data: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let payload = data.to_string();
        let len = payload.len() as u32;
        self.socket.write_all(&opcode.to_le_bytes())?;
        self.socket.write_all(&len.to_le_bytes())?;
        self.socket.write_all(payload.as_bytes())?;
        Ok(())
    }

    fn recv(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut header = [0u8; 8];
        self.socket.read_exact(&mut header)?;
        let len = u32::from_le_bytes(header[4..8].try_into()?) as usize;
        if len > 64 * 1024 {
            return Err(format!("IPC frame too large ({len} bytes)").into());
        }
        let mut buf = vec![0u8; len];
        self.socket.read_exact(&mut buf)?;
        let val = serde_json::from_slice(&buf)?;
        Ok(val)
    }

    fn set_activity(&mut self, activity: Activity) -> Result<(), Box<dyn std::error::Error>> {
        let data = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": std::process::id(),
                "activity": activity
            },
            "nonce": Uuid::new_v4().to_string()
        });
        self.send(1, &data)
    }

    fn clear_activity(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let data = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": std::process::id(),
                "activity": null
            },
            "nonce": Uuid::new_v4().to_string()
        });
        self.send(1, &data)
    }
}

#[derive(Serialize, Clone)]
struct Activity<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamps: Option<Timestamps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assets: Option<Assets<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buttons: Option<Vec<Button<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    activity_type: Option<ActivityType>,
}

#[derive(Serialize, Clone)]
struct Timestamps {
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<i64>,
}

#[derive(Serialize, Clone)]
struct Assets<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    large_image: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    large_text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    small_image: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    small_text: Option<&'a str>,
}

#[derive(Serialize, Clone)]
struct Button<'a> {
    label: &'a str,
    url: &'a str,
}

#[derive(Serialize_repr, Clone)]
#[repr(u8)]
enum ActivityType {
    Listening = 2,
}

fn truncate(s: &str, max: usize) -> &str {
    let mut len = max.min(s.len());
    while !s.is_char_boundary(len) {
        len -= 1;
    }
    &s[..len]
}

fn format_time(seconds: f64) -> String {
    let total = seconds as u64;
    let mins = total / 60;
    let secs = total % 60;
    format!("{mins}:{secs:02}")
}

fn try_connect(ipc_path: Option<&str>) -> Option<DiscordIpc> {
    let mut delay = Duration::from_secs(1);
    for _ in 0..5 {
        match DiscordIpc::connect(ipc_path) {
            Ok(client) => {
                println!("[synkmusic-rpc] Connected to Discord IPC");
                return Some(client);
            }
            Err(e) => {
                eprintln!(
                    "[synkmusic-rpc] Failed to connect to Discord ({}). Retrying in {}s...",
                    e,
                    delay.as_secs()
                );
                std::thread::sleep(delay);
                delay = (delay * 2).min(MAX_RECONNECT_DELAY);
            }
        }
    }
    eprintln!("[synkmusic-rpc] Could not connect after retries, will try again on next update");
    None
}

fn discord_ipc_loop(mut rx: mpsc::Receiver<RpcMessage>, _ipc_path: Option<String>) {
    #[cfg(unix)]
    let ipc_path_ref = _ipc_path.as_deref();
    #[cfg(windows)]
    let ipc_path_ref: Option<&str> = None;
    let mut client: Option<DiscordIpc> = None;
    let mut last_update: Option<(String, String, Option<String>)> = None;

    while let Some(msg) = rx.blocking_recv() {
        if client.is_none() {
            client = try_connect(ipc_path_ref);
            if client.is_none() {
                continue;
            }
        }

        match msg {
            RpcMessage::Update {
                track,
                artist,
                album,
                elapsed,
                duration,
                cover_url,
                track_url,
                player_status,
            } => {
                let is_paused = player_status.as_deref() == Some("paused");
                let key = (track.clone(), artist.clone(), player_status.clone());

                if last_update.as_ref() == Some(&key) {
                    continue;
                }
                last_update = Some(key);

                let details = truncate(&track, 128);

                let state_text = if is_paused && duration > 0.0 {
                    format!(
                        "by {} · Paused ({})",
                        truncate(&artist, 100),
                        format_time(elapsed)
                    )
                } else if is_paused {
                    format!("by {} · Paused", truncate(&artist, 114))
                } else {
                    format!("by {}", truncate(&artist, 124))
                };

                let large_image = cover_url
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .unwrap_or("synkmusic_logo");

                let large_hover = album
                    .as_deref()
                    .filter(|a| !a.is_empty())
                    .unwrap_or(&track);

                let (small_img, small_txt) = match player_status.as_deref() {
                    Some("paused") => ("https://share.synkteam.uk/i/pause.png", "Paused"),
                    Some("playing") => ("https://share.synkteam.uk/i/play.png", "Playing"),
                    _ => ("synkmusic_logo", "SYNK Music"),
                };

                let assets = Assets {
                    large_image: Some(large_image),
                    large_text: Some(truncate(large_hover, 128)),
                    small_image: Some(small_img),
                    small_text: Some(small_txt),
                };

                let timestamps = if !is_paused && duration > 0.0 && elapsed >= 0.0 && elapsed.is_finite() && duration.is_finite() {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    let start = now - elapsed as i64;
                    let end = start + duration as i64;
                    Some(Timestamps {
                        start: Some(start),
                        end: Some(end),
                    })
                } else {
                    None
                };

                let mut buttons = Vec::new();
                if let Some(url) = track_url.as_deref().filter(|u| !u.is_empty() && u.len() <= 512)
                {
                    buttons.push(Button {
                        label: "Listen on SYNK Music",
                        url,
                    });
                }
                buttons.push(Button {
                    label: "Open SYNK Music",
                    url: SYNKMUSIC_URL,
                });

                let activity = Activity {
                    state: Some(&state_text),
                    details: Some(details),
                    timestamps,
                    assets: Some(assets),
                    buttons: if buttons.is_empty() { None } else { Some(buttons) },
                    activity_type: Some(ActivityType::Listening),
                };

                if let Some(ref mut c) = client {
                    if let Err(e) = c.set_activity(activity) {
                        eprintln!("[synkmusic-rpc] Lost Discord connection ({}), will reconnect", e);
                        client = None;
                    }
                }
            }
            RpcMessage::Clear => {
                last_update = None;
                if let Some(ref mut c) = client {
                    if let Err(e) = c.clear_activity() {
                        eprintln!("[synkmusic-rpc] Lost Discord connection ({}), will reconnect", e);
                        client = None;
                    }
                }
            }
        }
    }

    if let Some(ref mut c) = client {
        let _ = c.close();
    }
}

fn wait_for_enter() {
    use std::io::{self, Read};
    println!("\nPress Enter to exit...");
    let _ = io::stdin().read(&mut [0u8]);
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    #[cfg(unix)]
    let ipc_path = args.ipc_path;
    #[cfg(windows)]
    let ipc_path: Option<String> = None;

    if let Err(e) = run(args.port, ipc_path).await {
        eprintln!("[synkmusic-rpc] Fatal error: {}", e);
        wait_for_enter();
    }
}

async fn run(port: u16, ipc_path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SYNK Music Discord RPC v1.0 ===");
    println!("NOTE: This is a WIP, expect bugs and occasional issues.");

    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            return Err(format!(
                "Port {} is already in use. Is another instance running?\n\
                 Try a different port with: synkmusic-rpc -p <port>",
                port
            )
            .into());
        }
        Err(e) => return Err(e.into()),
    };

    println!("Listening on ws://127.0.0.1:{}", port);

    let (tx, rx) = mpsc::channel(32);

    std::thread::spawn(move || discord_ipc_loop(rx, ipc_path));

    let accept_loop = async {
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("[synkmusic-rpc] Accept error: {}", e);
                    continue;
                }
            };

            println!("[synkmusic-rpc] Connection from {}", addr);
            let tx = tx.clone();

            tokio::spawn(async move {
                let ws_stream = match accept_async(stream).await {
                    Ok(ws) => ws,
                    Err(e) => {
                        eprintln!("[synkmusic-rpc] WS handshake failed: {}", e);
                        return;
                    }
                };

                let (_, mut read) = ws_stream.split();

                while let Some(result) = read.next().await {
                    let msg = match result {
                        Ok(msg) => msg,
                        Err(e) => {
                            eprintln!("[synkmusic-rpc] WS read error from {}: {}", addr, e);
                            break;
                        }
                    };

                    if msg.is_close() {
                        break;
                    }

                    if !msg.is_text() {
                        continue;
                    }

                    let text = match msg.into_text() {
                        Ok(t) => t,
                        Err(e) => {
                            eprintln!("[synkmusic-rpc] Invalid text frame from {}: {}", addr, e);
                            continue;
                        }
                    };

                    match serde_json::from_str::<RpcMessage>(&text) {
                        Ok(rpc_msg) => {
                            if tx.send(rpc_msg).await.is_err() {
                                eprintln!("[synkmusic-rpc] IPC channel closed, dropping connection from {}", addr);
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("[synkmusic-rpc] Bad message from {}: {}", addr, e);
                        }
                    }
                }

                println!("[synkmusic-rpc] Disconnected: {}", addr);
                let _ = tx.send(RpcMessage::Clear).await;
            });
        }
    };

    tokio::select! {
        _ = accept_loop => {}
        _ = tokio::signal::ctrl_c() => {
            println!("\n[synkmusic-rpc] Shutting down, clearing Discord activity...");
            let _ = tx.send(RpcMessage::Clear).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    Ok(())
}
