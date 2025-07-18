use futures_util::stream::{SplitSink, SplitStream};
use log::{debug, error, info, warn};
use serde_json::{Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc};
use tokio::sync::watch::{Receiver};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::http::Uri;

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
use serde_json::json;                // <-- для макро json!
use futures_util::{StreamExt, SinkExt}; // SplitSink/Stream нужны
use chrono::Utc;
use alor_http::AlorHttp;             // <-- HTTP-клиент с JWT

use futures_core::Stream;
//use crate::{WsEvent, Quote, Bar}; 
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::spawn;
use futures_util::stream::{BoxStream, StreamExt as _};
use tokio_tungstenite::tungstenite::Error as WsError;
use crate::structs::history_data::HistoryDataResponse;
use tokio::sync::RwLock;
use crate::types::{SubscriptionInfo, OpCode, WsEvent, Quote, Bar};
use tokio::sync::watch::Receiver as ShutdownReceiver;



pub struct WebSocketState {
	pub write_stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
	//read_stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
	read_stream: BoxStream<'static, Result<Message, WsError>>,
	 // 1) «Сырая» очередь для (guid, JSON)
    raw_tx: mpsc::Sender<(Uuid, Value)>,
    raw_rx: mpsc::Receiver<(Uuid, Value)>,


    //alive_subs: HashMap<Uuid, String>,   // guid -> raw JSON
	last_bar_ts:  HashMap<(String, String), i64>,
    http: Arc<AlorHttp>,
	alive_subs: Arc<RwLock<HashMap<Uuid, SubscriptionInfo>>>,

    // 3) Готовые события
    event_tx: mpsc::Sender<WsEvent>,
    pub event_rx: mpsc::Receiver<WsEvent>,
	shutdown_rx: ShutdownReceiver<bool>,
}


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


impl Stream for WebSocketState {
    type Item = WsEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // poll_recv есть прямо у Receiver
        Pin::new(&mut self.event_rx).poll_recv(cx)
    }
}


impl WebSocketState {
	pub async fn new_pipeline(
        url: Uri,
        http: Arc<AlorHttp>,
		shutdown_rx: ShutdownReceiver<bool>,
    ) -> anyhow::Result<Self> {
        let (write, read) = Self::connect(url).await?;
        let read_stream   = read.boxed();

        // 1) Raw JSON channel
        let (raw_tx, raw_rx) = mpsc::channel(256);

        // 2) Subscriptions map
        let alive_subs = Arc::new(RwLock::new(HashMap::new()));

        // 3) Final events channel
        let (event_tx, event_rx) = mpsc::channel(256);

        let mut me = Self {
            write_stream: write,
            read_stream,

            raw_tx,
            raw_rx,

            alive_subs,
            last_bar_ts: HashMap::new(),
            http,

            event_tx,
            event_rx,
			shutdown_rx,
        };

        // spawn both stages
        me.spawn_reader_loop();
        me.spawn_dispatcher_loop();
        Ok(me)
    }


	async fn connect(url: Uri) -> anyhow::Result<WsPair> {
		let (ws, _) = connect_async(url).await?;
		Ok(ws.split())
	}


	fn spawn_reader_loop(&mut self) {
		let mut read = std::mem::replace(&mut self.read_stream, futures_util::stream::empty().boxed());
		let raw_tx = self.raw_tx.clone();
		let mut shutdown = self.shutdown_rx.clone();

		tokio::spawn(async move {
			loop {
				tokio::select! {
					biased;
					_ = shutdown.changed() => {
						if *shutdown.borrow() {
							info!("reader_loop: shutdown signal received");
							break;
						}
					}
					msg_opt = read.next() => match msg_opt {
						Some(Ok(Message::Text(txt))) => {
							// Скипаем ACK без data
							if let Ok(v) = serde_json::from_str::<Value>(&txt) {
								if v.get("data").is_none() {
									continue;
								}
								if let Some(guid_s) = v.get("guid").and_then(Value::as_str) {
									if let Ok(guid) = Uuid::parse_str(guid_s) {
										let _ = raw_tx.send((guid, v)).await;
									}
								}
							}
						}
						Some(Ok(Message::Ping(p))) => {
							debug!("reader_loop: Ping {:?}", p);
						}
						Some(Ok(Message::Pong(p))) => {
							debug!("reader_loop: Pong {:?}", p);
						}
						Some(Ok(Message::Close(frame))) => {
							info!("reader_loop: connection closed by server: {:?}", frame);
							break;
						}
						Some(Ok(_)) => { /* Binary, Frame — игнор */ }
						Some(Err(e)) => {
							error!("reader_loop: ws error: {}", e);
							break;
						}
						None => {
							info!("reader_loop: stream ended");
							break;
						}
					}
				}
			}
			info!("reader_loop: finished");
		});
	}




	fn spawn_dispatcher_loop(&mut self) {
		// вынимаем raw_rx
		let mut raw_rx = std::mem::replace(&mut self.raw_rx, mpsc::channel(1).1);
		let subs_map = Arc::clone(&self.alive_subs);
		let event_tx = self.event_tx.clone();

		// для каждого guid храним последний ts и последний Value
		let mut last_ts: HashMap<Uuid, i64> = HashMap::new();
		let mut prev_bar_value: HashMap<Uuid, Value> = HashMap::new();
		let mut last_quote: HashMap<Uuid, Quote> = HashMap::new(); // Добавим для хранения последней котировки

		tokio::spawn(async move {
			while let Some((guid, v)) = raw_rx.recv().await {
				// пытаемся достать SubscriptionInfo
				if let Some(info) = subs_map.read().await.get(&guid).cloned() {
					// пробуем смэпить Value → WsEvent
					if let Some(ev) = WsEvent::try_from_json(guid, &v, &info) {
						// если это Bar — то обработаем бар
						if let WsEvent::Bar(bar) = &ev {
							let ts = bar.ts;
							let prev_ts = last_ts.get(&guid).copied().unwrap_or(0);

							if ts > prev_ts {
								// новый бар — отправляем его дальше
								event_tx.send(ev.clone()).await.unwrap();
								last_ts.insert(guid, bar.ts);
								prev_bar_value.insert(guid, v.clone());
								debug!("Dispatching Bar: {:?}", ev);
							} else {
								debug!("Skipping duplicate/partial bar ts={} for guid={}", ts, guid);
							}
						}
						// если это Quote — фильтруем повторяющиеся котировки
						else if let WsEvent::Quote(q) = &ev {
							if let Some(last_q) = last_quote.get(&guid) {
								// Сравниваем цену, если она изменилась, выводим
								if last_q.last_price != q.last_price || last_q.bid != q.bid || last_q.ask != q.ask {
									// Новая котировка, выводим и обновляем last_quote
									debug!("Dispatching Quote: {:?}", ev);
									event_tx.send(ev.clone()).await.unwrap();
									last_quote.insert(guid, q.clone());
								} else {
									debug!("Skipping duplicate quote for guid={}", guid);
								}
							} else {
								// Если предыдущей котировки нет — выводим первую
								debug!("Dispatching initial Quote: {:?}", ev);
								event_tx.send(ev.clone()).await.unwrap();
								last_quote.insert(guid, q.clone());
							}
						}
						// Если это не Bar и не Quote, просто передаем
						else {
							let _ = event_tx.send(ev).await;
						}
					}
				}
			}
			info!("dispatcher_loop: finished");
		});
	}


/// Пересылаем текущие подписки с новым token
    async fn reauthorize(&mut self) -> anyhow::Result<()> {
        info!("reauthorizing");
        let jwt = self.http.jwt().await;
        // Берем read-lock на мапу подписок
        let subs_map = self.alive_subs.read().await;

        for (guid, info) in subs_map.iter() {
            // Собираем JSON заново по типу подписки
            let req = match info.kind {
                OpCode::QuotesSubscribe => json!({
                    "opcode":   "QuotesSubscribe",
                    "code":     info.symbol,
                    "exchange": info.exchange,
                    "token":    jwt,
                    "guid":     guid.to_string(),
                }),
                OpCode::BarsGetAndSubscribe => {
                    // У unwrap безопасен, т.к. для Bar tf всегда Some
                    let tf = info.tf.as_ref().unwrap();
                    json!({
                        "opcode":      "BarsGetAndSubscribe",
                        "code":        info.symbol,
                        "tf":          tf,
                        "exchange":    info.exchange,
                        "token":       jwt,
                        "guid":        guid.to_string(),
                    })
                }
            };

            // Отправляем текстовый фрейм; .into() превращает String → Utf8Bytes
            self.write_stream
                .send(Message::Text(req.to_string().into()))
                .await?;
        }

        Ok(())
    }

    pub async fn subscribe_quotes(&mut self, code: &str, exchange: &str) -> anyhow::Result<()> {
		let guid = Uuid::new_v4();
		let jwt  = self.http.jwt().await;
		let req  = json!({
			"opcode":   "QuotesSubscribe",
			"code":     code,
			"exchange": exchange,
			"token":    jwt,
			"guid":     guid.to_string(),
		});
		self.write_stream.send(Message::Text(req.to_string().into())).await?;

		let info = SubscriptionInfo {
			symbol:   code.to_string(),
			exchange: exchange.to_string(),
			kind:     OpCode::QuotesSubscribe,
			tf:       None,
		};
		self.alive_subs.write().await.insert(guid, info);
		Ok(())
	}

    pub async fn subscribe_bars(
		&mut self,
		code: &str,
		tf: &str,
		exchange: &str,
		from_timestamp: i64,      // начальное время в UNIX timestamp
	) -> anyhow::Result<()> {
		let guid = Uuid::new_v4();
		let jwt = self.http.jwt().await;  // Получаем токен для авторизации
		let to_timestamp = Utc::now().timestamp();  // Текущее время для завершения диапазона

		// Если нужны только новые бары, можно задать skipHistory = true, но при этом не обязательно указывать `from`
		let req = json!({
			"opcode": "BarsGetAndSubscribe",
			"code": code,                // Символ инструмента
			"tf": tf,                    // Таймфрейм
			"from": from_timestamp,      // Начало диапазона (в UNIX timestamp)
			"to": to_timestamp,          // Конец диапазона (текущее время)
			"skipHistory": false,        // Если false, запрашиваются бары с указанного времени, если true — только новые
			"exchange": exchange,        // Биржа (например, "MOEX")
			"format": "Simple",          // Формат данных
			"frequency": 100,            // Частота обновления (можно настроить)
			"token": jwt,                // Авторизационный токен
			"guid": guid.to_string(),    // Уникальный идентификатор для подписки
		});

		// Отправляем запрос на сервер
		self.write_stream.send(Message::Text(req.to_string().into())).await?;

		// Сохраняем информацию о подписке
		self.alive_subs.write().await.insert(guid, SubscriptionInfo {
			symbol: code.to_string(),
			exchange: exchange.to_string(),
			kind: OpCode::BarsGetAndSubscribe,
			tf: Some(tf.to_string()),
		});

		Ok(())
	}
}