use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;

const APP_ID: &str = "1489549668810493993";
const PORT: u16 = 19836;
const ENCORE_URL: &str = "https://encore.synkteam.uk";

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

fn discord_ipc_loop(mut rx: mpsc::Receiver<RpcMessage>) {
    let mut client = DiscordIpcClient::new(APP_ID).expect("Failed to create IPC client");
    let mut connected = false;

    while let Some(msg) = rx.blocking_recv() {
        if !connected {
            if client.connect().is_ok() {
                connected = true;
                println!("[encore-rpc] Connected to Discord IPC");
            } else {
                eprintln!("[encore-rpc] Failed to connect to Discord. Retrying next update...");
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
                let details = truncate(&track, 128);
                let state_text = format!("by {}", truncate(&artist, 124));

                let large_image = cover_url
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .unwrap_or("encore_logo");

                let large_hover = album
                    .as_deref()
                    .filter(|a| !a.is_empty())
                    .unwrap_or(&track);
                
                let (small_img, small_txt) = match player_status.as_deref() {
                    Some("paused") => ("https://share.synkteam.uk/i/pause.png", "Paused"),
                    Some("playing") => ("https://share.synkteam.uk/i/play.png", "Playing"),
                    _ => ("encore_logo", "Encore"),
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

                if player_status.as_deref() != Some("paused") && duration > 0.0 {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    let start = now - elapsed as i64;
                    let end = start + duration as i64;
                    activity = activity.timestamps(Timestamps::new().start(start).end(end));
                }

                let mut buttons = Vec::new();
                if let Some(url) = track_url.as_deref().filter(|u| !u.is_empty() && u.len() <= 512) {
                    buttons.push(Button::new("Listen on Encore", url));
                }
                buttons.push(Button::new("Open Encore", ENCORE_URL));

                activity = activity.buttons(buttons);

                if client.set_activity(activity).is_err() {
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
    println!("=== Encore Discord RPC v1.0 ===");
    println!("NOTE: This is a WIP, expect bugs and occasional issues.");
    println!("Listening on ws://127.0.0.1:{}", PORT);

    let (tx, rx) = mpsc::channel(32);

    std::thread::spawn(move || discord_ipc_loop(rx));

    let listener = TcpListener::bind(("127.0.0.1", PORT)).await?;

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(res) => res,
            Err(e) => {
                eprintln!("[encore-rpc] Accept error: {}", e);
                continue;
            }
        };

        println!("[encore-rpc] Connection from {}", addr);
        let tx = tx.clone();

        tokio::spawn(async move {
            let ws_stream = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("[encore-rpc] WS handshake failed: {}", e);
                    return;
                }
            };

            let (_, mut read) = ws_stream.split();

            while let Some(Ok(msg)) = read.next().await {
                if msg.is_text() {
                    if let Ok(text) = msg.into_text() {
                        if let Ok(rpc_msg) = serde_json::from_str::<RpcMessage>(&text) {
                            let _ = tx.send(rpc_msg).await;
                        }
                    }
                } else if msg.is_close() {
                    break;
                }
            }

            println!("[encore-rpc] Disconnected: {}", addr);
            let _ = tx.send(RpcMessage::Clear).await;
        });
    }
}