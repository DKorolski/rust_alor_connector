use futures_util::stream::{SplitSink, SplitStream};
use log::{debug, error, info, warn};
use serde_json::{Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc};
use tokio::sync::watch::{channel, Receiver};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::http::Uri;

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
use serde_json::json;                // <-- для макро json!
use futures_util::{StreamExt, SinkExt}; // SplitSink/Stream нужны
use chrono::Utc;
use alor_http::AlorHttp;             // <-- HTTP-клиент с JWT


pub struct WebSocketState {
	pub write_stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
	read_stream:  SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    alive_subs: HashMap<Uuid, String>,   // guid -> raw JSON
    http: Arc<AlorHttp>,
}
use crate::structs::history_data::HistoryDataResponse;

type WsSink = 
    futures_util::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >;
type WsStream =
    futures_util::stream::SplitStream<
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    >;

type WsPair = (
		futures_util::stream::SplitSink<
			WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
			Message,
		>,
		futures_util::stream::SplitStream<
			WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
		>,
	);


impl WebSocketState {
	async fn socket_listener(mut stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>, _api_response_tx: mpsc::Sender<Value>, mut shutdown_rx: Receiver<bool>, callback: fn (&Value)) {
		debug!("Starting WebSocketListener");

		let mut prev_bar: Option<Value> = None;
		loop {
			tokio::select! {
				// Ожидаем сообщение от WebSocket ИЛИ сигнал завершения
				msg_result = stream.next() => {
					match msg_result {
						Some(Ok(msg)) => {
							match msg {
								Message::Text(txt) => {
									match serde_json::from_str::<Value>(&txt) {
										Ok(api_response) => {
											debug!("Получен JSON: {:?}", api_response);
											if api_response.get("data").is_none() {
												continue;
											}

											if prev_bar.is_none() {
												prev_bar = Some(api_response);
												continue;
											} else {
												let prev_bar_data = prev_bar.clone().unwrap();

												let current_bar_secconds = api_response["data"]["time"].clone().as_u64().unwrap();
												let prev_bar_secconds = prev_bar_data["data"]["time"].clone().as_u64().unwrap();

												if current_bar_secconds == prev_bar_secconds {  // обновленная версия текущего бара
													prev_bar = Some(api_response);
												} else if current_bar_secconds > prev_bar_secconds {
													debug!("websocket_handler: OnNewBar {:?}", prev_bar_data);
													// raise callback on previous bar
													callback(&prev_bar.unwrap());
													// set current bar as previous bar
													prev_bar = Some(api_response);
												}
											}
											// if api_response_tx.send(api_response).await.is_err() {
											// 	error!("Не удалось отправить полученное сообщение (канал закрыт).");
											// 	break; // Выход, если внешний приемник закрыт
											// }
										}
										Err(e) => {
											warn!("Не JSON: {}. Данные: {}", e, txt);
											 // Можно отправить "сырое" сообщение или ошибку, если нужно
											 // let raw_response = ApiResponse { event: "raw_text".into(), data: Some(serde_json::Value::String(txt)), .. };
											 // if api_response_tx.send(raw_response).await.is_err() { break; }
										}
									}
								}
								Message::Binary(bin) => { // Обработка бинарных данных
									 debug!("Бинарные данные: {} байт", bin.len());
									 // Если нужно передать бинарные данные наружу, измените тип канала api_response_tx
								}
								Message::Ping(_) => {
									debug!("Получен Ping")
								},
								Message::Pong(_) => {
									// debug!("Получен Pong");
								},
								Message::Close(frame) => {
									debug!("Соединение закрыто сервером: {:?}", frame);
									break;
								}
								 Message::Frame(_) => warn!("Неожиданный Frame"),
							}
						},
						Some(Err(e)) => {
							error!("Ошибка чтения: {}", e);
							// Попытка отправить ошибку наружу?
							//  let err_response = Value { event: "read_error".into(), error_message: Some(e.to_string()), error_code: Some(-1), data:None, channel: None, symbol: None };
							//  let _ = api_response_tx.send(err_response).await; // Игнорируем ошибку отправки здесь
							break;
						},
						None => {
							info!("Поток чтения завершен.");
							 // Попытка отправить сообщение о дисконнекте?
							 // let close_response = Value { event: "disconnected".into(), error_code: Some(-2), error_message: None, data:None, channel: None, symbol: None };
							 // let _ = api_response_tx.send(close_response).await;
							break;
						}
					}
				}
				// Проверяем сигнал завершения
				_ = shutdown_rx.changed() => {
					 if *shutdown_rx.borrow() {
						info!("Получен сигнал завершения.");
						panic!("socket closed");
						break;
					 }
				}
			}
		}
		info!("Задача чтения завершена.");
		// api_response_tx закроется автоматически при выходе из функции
	}

	pub async fn with_http(
        url: Uri,
        http: Arc<AlorHttp>,
        callback: fn(&Value),
    ) -> anyhow::Result<Self> {
        let (write, read) = Self::connect(url.clone()).await?;
        let mut me = Self {
            write_stream: write,
            read_stream:  read,
            alive_subs:   HashMap::new(),
            http,
        };

        // первый запуск listener
		//info!("запуск listener");
        //me.spawn_listener(callback);
        Ok(me)
    }



	async fn connect(url: Uri) -> anyhow::Result<WsPair> {
		let (ws, _) = connect_async(url).await?;
		Ok(ws.split())
	}


	pub async fn poll_next(&mut self, callback: fn(&Value)) -> anyhow::Result<()> {
        if let Some(msg) = self.read_stream.next().await {
            if let Ok(Message::Text(txt)) = msg {
                if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                    callback(&v);
                }
            }
        }
        Ok(())
    }

	pub async fn new(url: Uri, callback: fn(&Value)) -> Self {
		// создаём временный AlorHttp только для JWT-чанка (истечёт – reconnect сработает)
		let dummy = Arc::new(
			AlorHttp::new(std::env::var("REFRESH_TOKEN").unwrap(), false)
				.await
				.expect("create http"),
		);

		// подключаем WebSocket с использованием существующего клиента
		info!("Подключение к Alor WebSocket...");
		let ws_state = Self::with_http(url, dummy, callback)
			.await
			.expect("Не удалось подключиться");

		info!("Успешно подключено!");
		ws_state
	}

	async fn reauthorize(&mut self) -> anyhow::Result<()> {
		info!("reauthorizing");
        let jwt = self.http.jwt().await;
        // nothing else to send for /ws — достаточно подставить token в каждую подписку
        for json in self.alive_subs.values() {
            let mut v: Value = serde_json::from_str(json)?;
            v["token"] = jwt.clone().into();
            self.write_stream.send(Message::Text(v.to_string().into())).await?;
        }
        Ok(())
    }

    pub async fn subscribe_quotes(
		&mut self,
		code: &str,
		exchange: &str,
	) -> anyhow::Result<()> {
		let jwt = self.http.jwt().await;
		let req = json!({
			"opcode": "QuotesSubscribe",
			"code": code,
			"exchange": exchange,
			"token": jwt,
			"guid": Uuid::new_v4().to_string(),
		});
		let guid = req["guid"].as_str().unwrap().to_string();

		self.write_stream
			.send(Message::Text(req.to_string().into()))
			.await?;

		self.alive_subs.insert(Uuid::parse_str(&guid)?, req.to_string());
		Ok(())
	}
}