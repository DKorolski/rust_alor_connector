//! Получаем best-bid/ask + last по инструменту IMOEXF через Stream-API.

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
    // .env не обязателен, но удобно хранить REFRESH_TOKEN здесь
    let _ = dotenvy::dotenv();

    // 1) HTTP-клиент с авто-refresh JWT
    let refresh = std::env::var("REFRESH_TOKEN")
        .expect("Положите REFRESH_TOKEN в .env либо в переменные окружения");
    let http = Arc::new(AlorHttp::new(refresh, /*demo=*/ false).await?);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    // 2) WebSocket + Quotes-подписка
    let ws_url: Uri = "wss://api.alor.ru/ws".parse()?;
    //let mut ws = WebSocketState::with_http(ws_url, http.clone(), no_callback).await?;
    let mut ws = WebSocketState::new_pipeline(ws_url, http.clone(), shutdown_rx).await?;

    ws.subscribe_quotes("IMOEXF", "MOEX").await?;

    println!("Listening for quotes …  Ctrl-C для выхода");
  

    // 3) Читаем события из Stream
    while let Some(ev) = ws.next().await {
        if let WsEvent::Quote(q) = ev {
            println!("Q {}: {}×{}  last {} vol {:?} @{}",q.symbol, q.bid, q.ask, q.last_price, q.volume, q.last_price_timestamp);
        }
    }

    // никому не мешаем вернуть Result
    Ok(())
}
