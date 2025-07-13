// examples/ws_subscribe.rs
//[2025-07-13T10:52:58Z DEBUG alor_rust::structs::websocket_state] Получен JSON: Object {"data": Object {"close": Number(2646.0), "high": Number(2646.0), "low": Number(2644.5), "open": Number(2645.0), "time": Number(1752262260), "volume": Number(219)}, "guid": String("0effa963-b0e6-4069-a5f5-a21144361967")}
//[2025-07-13T10:52:58Z DEBUG alor_rust::structs::websocket_state] websocket_handler: OnNewBar Object {"data": Object {"close": Number(2645.0), "high": Number(2648.0), "low": Number(2644.5), "open": Number(2647.5), "time": Number(1752262200), "volume": Number(830)}, "guid": String("0effa963-b0e6-4069-a5f5-a21144361967")}
//[BAR] Object {"close": Number(2645.0), "high": Number(2648.0), "low": Number(2644.5), "open": Number(2647.5), "time": Number(1752262200), "volume": Number(830)}
//WebSocket-подписка Через WebSocketState напрямую let mut ws = WebSocketState::new(ws_url, on_bar).await;
//Через встроенный в AlorRust WS-механизм let mut api = AlorRust::new(&refresh_token, false, on_bar).await;
//WebSocketState::socket_listener читает все входящие JSON-сообщения (они приходят как Value
//Всевозможные «opcode» (типы событий) зависят от того, какие подписки

use anyhow::Result;
use futures_util::SinkExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::http::Uri;
use reqwest::Client; 


// Your WS client
use alor_connector::structs::websocket_state::WebSocketState;
use env_logger;

fn init_logger() {
    env_logger::builder()
        .filter_module("alor_connector::structs::websocket_state", log::LevelFilter::Debug)
        .init();
}

// Callback for new bars
fn on_bar(msg: &Value) {
    println!("[BAR] {:?}", msg["data"]);
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logger();
    let refresh = std::env::var("REFRESH_TOKEN")?;
    let auth: serde_json::Value = Client::new()
        .post("https://oauth.alor.ru/refresh")
        .json(&serde_json::json!({ "token": refresh }))
        .send().await?
        .json().await?;
    let access = auth["AccessToken"]
        .as_str().unwrap()
        .to_string();
    // Use the proper /ws endpoint
    let ws_url: Uri = "wss://api.alor.ru/ws".parse()?;
    let mut ws = WebSocketState::new(ws_url, on_bar).await;

    // Build the subscription request
    let subscribe = json!({
        "opcode":    "BarsGetAndSubscribe",
        "code":      "IMOEXF",
        "tf":        "60",
        "from":      (chrono::Utc::now() - chrono::Duration::hours(150)).timestamp(),
        "skipHistory": false,
        "exchange":  "MOEX",
        "instrumentGroup": "RFUD",
        "format":    "Simple",
        "frequency": 100,
        "guid":      uuid::Uuid::new_v4().to_string(),
        "token":     access,
    });

    // Send it
    ws.write_stream
      .send(Message::Text(subscribe.to_string().into()))
      .await?;

    println!("✅ Subscribed – listening for bars…");
    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
