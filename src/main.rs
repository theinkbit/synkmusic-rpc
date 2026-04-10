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

const DISCORD_APP_ID: &str = "1489549668810493993";
const WS_PORT: u16 = 19836;
const SYNKMUSIC_URL: &str = "https://synkmusic.synkteam.uk";

const ICON_PAUSE: &str = "https://share.synkteam.uk/i/pause.png";
const ICON_PLAY: &str = "https://share.synkteam.uk/i/play.png";
const LOGO_ASSET: &str = "synkmusic_logo";

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum RpcMessage {
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
    let mut client =
        DiscordIpcClient::new(DISCORD_APP_ID).expect("failed to create Discord IPC client");
    let mut connected = false;

    while let Some(msg) = rx.blocking_recv() {
        if !connected {
            if client.connect().is_ok() {
                connected = true;
                println!("[synkmusic-rpc] Connected to Discord IPC");
            } else {
                eprintln!("[synkmusic-rpc] Discord not available, retrying on next update");
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
                    .unwrap_or(LOGO_ASSET);

                let large_hover = album
                    .as_deref()
                    .filter(|a| !a.is_empty())
                    .unwrap_or(&track);

                let (small_img, small_txt) = match player_status.as_deref() {
                    Some("paused") => (ICON_PAUSE, "Paused"),
                    Some("playing") => (ICON_PLAY, "Playing"),
                    _ => (LOGO_ASSET, "SynkMusic"),
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
                if let Some(url) =
                    track_url
                        .as_deref()
                        .filter(|u| !u.is_empty() && u.len() <= 512)
                {
                    buttons.push(Button::new("Listen on SynkMusic", url));
                }
                buttons.push(Button::new("Open SynkMusic", SYNKMUSIC_URL));
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
    println!("[synkmusic-rpc] Listening on ws://127.0.0.1:{WS_PORT}");

    let (tx, rx) = mpsc::channel(32);
    std::thread::spawn(move || discord_ipc_loop(rx));

    let listener = TcpListener::bind(("127.0.0.1", WS_PORT)).await?;

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("[synkmusic-rpc] Accept error: {e}");
                continue;
            }
        };

        println!("[synkmusic-rpc] Connection from {addr}");
        let tx = tx.clone();

        tokio::spawn(async move {
            let ws = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("[synkmusic-rpc] WebSocket handshake failed: {e}");
                    return;
                }
            };

            let (_, mut read) = ws.split();

            while let Some(Ok(msg)) = read.next().await {
                if msg.is_text() {
                    if let Ok(text) = msg.into_text() {
                        if let Ok(rpc) = serde_json::from_str::<RpcMessage>(&text) {
                            let _ = tx.send(rpc).await;
                        }
                    }
                } else if msg.is_close() {
                    break;
                }
            }

            println!("[synkmusic-rpc] Disconnected: {addr}");
            let _ = tx.send(RpcMessage::Clear).await;
        });
    }
}
