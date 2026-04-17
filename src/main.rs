use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;

const APP_ID: &str = "1489549668810493993";
const PORT: u16 = 19836;
const SYNKMUSIC_URL: &str = "https://synkmusic.com";
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

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

fn try_connect(client: &mut DiscordIpcClient) -> bool {
    let mut delay = Duration::from_secs(1);
    for _ in 0..5 {
        if client.connect().is_ok() {
            println!("[synkmusic-rpc] Connected to Discord IPC");
            return true;
        }
        eprintln!(
            "[synkmusic-rpc] Failed to connect to Discord. Retrying in {}s...",
            delay.as_secs()
        );
        std::thread::sleep(delay);
        delay = (delay * 2).min(MAX_RECONNECT_DELAY);
    }
    eprintln!("[synkmusic-rpc] Could not connect after retries, will try again on next update");
    false
}

fn discord_ipc_loop(mut rx: mpsc::Receiver<RpcMessage>) {
    let mut client = DiscordIpcClient::new(APP_ID).expect("Failed to create IPC client");
    let mut connected = false;

    while let Some(msg) = rx.blocking_recv() {
        if !connected {
            connected = try_connect(&mut client);
            if !connected {
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

                let assets = Assets::new()
                    .large_image(large_image)
                    .large_text(truncate(large_hover, 128))
                    .small_image(small_img)
                    .small_text(small_txt);

                let mut activity = Activity::new()
                    .activity_type(ActivityType::Listening)
                    .details(details)
                    .state(&state_text)
                    .assets(assets);

                if !is_paused && duration > 0.0 {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    let start = now - elapsed as i64;
                    let end = start + duration as i64;
                    activity = activity.timestamps(Timestamps::new().start(start).end(end));
                }

                let mut buttons = Vec::new();
                if let Some(url) = track_url.as_deref().filter(|u| !u.is_empty() && u.len() <= 512)
                {
                    buttons.push(Button::new("Listen on SYNK Music", url));
                }
                buttons.push(Button::new("Open SYNK Music", SYNKMUSIC_URL));

                activity = activity.buttons(buttons);

                if client.set_activity(activity).is_err() {
                    eprintln!("[synkmusic-rpc] Lost Discord connection, will reconnect");
                    connected = false;
                }
            }
            RpcMessage::Clear => {
                if client.clear_activity().is_err() {
                    connected = false;
                }
            }
        }
    }

    let _ = client.close();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SYNK Music Discord RPC v1.0 ===");
    println!("NOTE: This is a WIP, expect bugs and occasional issues.");
    println!("Listening on ws://127.0.0.1:{}", PORT);

    let (tx, rx) = mpsc::channel(32);

    std::thread::spawn(move || discord_ipc_loop(rx));

    let listener = TcpListener::bind(("127.0.0.1", PORT)).await?;

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

                while let Some(Ok(msg)) = read.next().await {
                    if msg.is_text() {
                        if let Ok(text) = msg.into_text() {
                            match serde_json::from_str::<RpcMessage>(&text) {
                                Ok(rpc_msg) => {
                                    let _ = tx.send(rpc_msg).await;
                                }
                                Err(e) => {
                                    eprintln!("[synkmusic-rpc] Bad message from {}: {}", addr, e);
                                }
                            }
                        }
                    } else if msg.is_close() {
                        break;
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
