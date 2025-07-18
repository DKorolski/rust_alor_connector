//! Пример подключения к Alor WebSocket и подписки на бары (Bars). 
// https://alor.dev/docs/api/websocket/data-subscriptions/BarsGetAndSubscribe

use anyhow::Result;
use http::Uri;
use std::sync::Arc;

use serde_json::Value;
use futures_util::StreamExt;

use alor_http::AlorHttp;
use alor_connector::{
    structs::websocket_state::WebSocketState,
    WsEvent,            // <-- наш enum
};

use http::Uri as HttpUri;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    // (необязательно) читаем .env, если он есть: REFRESH_TOKEN=xxxxx
    let _ = dotenvy::dotenv();

    // 1) Создаём HTTP-клиент с авто-refresh JWT
    let refresh = std::env::var("REFRESH_TOKEN")
        .expect("Положите REFRESH_TOKEN в .env или переменные окружения");
    let http = Arc::new(AlorHttp::new(refresh, /*demo=*/ false).await?);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    // 2) Подключаемся к WebSocket через новый helper
    let ws_url: Uri = "wss://api.alor.ru/ws".parse()?;
    let mut ws = WebSocketState::new_pipeline(ws_url, http.clone(), shutdown_rx).await?;

    // 3) Подписываемся на бары
    ws.subscribe_bars("IMOEXF", "60", "MOEX", 1752769680 ).await?;
    println!("Listening for quotes …  Ctrl-C для выхода");
    while let Some(ev) = ws.next().await {
        if let WsEvent::Bar(b) = ev {
            println!("BAR {} O={} H={} L={} C={} V={}",
                    b.ts, b.open, b.high, b.low, b.close, b.volume);
        }
    }
    Ok(())
}
