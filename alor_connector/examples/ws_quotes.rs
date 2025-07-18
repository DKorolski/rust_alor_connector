//! Пример подключения к Alor WebSocket и подписки на котировки (Quotes).

use anyhow::Result;
use http::Uri;
use std::sync::Arc;

use serde_json::Value;

use alor_http::AlorHttp;
use alor_connector::structs::websocket_state::WebSocketState;

/// Простой колл-бек: печатает всё, что приходит от сервера.
fn print_event(v: &Value) {
    if let Some(opcode) = v.get("opcode") {
        println!("{}: {}", opcode, v);
    } else {
        println!("{}", v);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // (необязательно) читаем .env, если он есть: REFRESH_TOKEN=xxxxx
    let _ = dotenvy::dotenv();

    // 1) Создаём HTTP-клиент с авто-refresh JWT
    let refresh = std::env::var("REFRESH_TOKEN")
        .expect("Положите REFRESH_TOKEN в .env или переменные окружения");
    let http = Arc::new(AlorHttp::new(refresh, /*demo=*/ false).await?);

    // 2) Подключаемся к WebSocket через новый helper
    let ws_url: Uri = "wss://api.alor.ru/ws".parse()?;
    let mut ws = WebSocketState::with_http(ws_url, http.clone(), print_event).await?;

    // 3) Подписываемся на котировки инструмента (best bid/ask + last)
    ws.subscribe_quotes("IMOEXF", "MOEX").await?;

    println!("Listening for quotes … Ctrl-C для выхода");

    // 4) Бесконечно читаем поток: poll_next() вызывает print_event
    loop {
        ws.next_with_callback(print_event).await?;
    }
}
