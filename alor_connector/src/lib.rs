use chrono::{DateTime, Utc};
use log::{debug, info, warn};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Url;
use serde_json::{Error, Number, Value};
use std::error::Error as StdError;
use std::collections::HashMap;
use std::sync::mpsc;
use chrono_tz::Tz;
use futures_util::{SinkExt};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Utf8Bytes;
pub use alor_http::{AlorHttp, Bar, AlorError};

pub mod structs {
    pub mod api_client;

    pub mod token_data;

    pub mod user_data;

    pub mod history_data;

    pub mod websocket_state;
}

pub use structs::history_data::HistoryDataResponse;

mod helpers {
}

use structs::api_client::*;
use structs::history_data::*;
use structs::token_data::*;
use structs::user_data::*;
use uuid::Uuid;

/// Formats the sum of two numbers as string.
pub struct AlorRust {
    token_data: TokenData,
    user_data: UserData,
    client: ApiClient,
    symbols: HashMap<String, Value>,
    exchanges: Vec<String>,
}

impl AlorRust {

    pub async fn new(refresh_token: &str, demo: bool, get_bar_callback: fn (&Value)) -> Self {
        info!("Initializing AlorRust, demo status: {demo}");
        let client = ApiClient::new(demo, get_bar_callback).await.unwrap();

        let mut api = AlorRust {
            token_data: TokenData {
                raw_response: None,
                token: None,
                refresh_token: refresh_token.to_string(),
                token_decoded: None,
                token_issued: 0,
                token_ttl: 60,
            },
            user_data: UserData {
                accounts: Vec::new(),
            },
            client,
            symbols: HashMap::new(),
            exchanges: vec!["MOEX".to_string(), "SPBX".to_string()],
        };

        api.get_jwt_token().await.unwrap();

        api
    }

    // internal

    async fn get_headers_vec(&mut self) -> HeaderMap {
        let data_json: Value = serde_json::from_str(self.get_jwt_token().await.unwrap().as_str()).unwrap();

        let mut headers = HeaderMap::new();

        headers.insert(
            "Content-Type",
            HeaderValue::from_str("application/json").unwrap(),
        );
        headers.insert(
            "Authorization",
            HeaderValue::from_str(
                format!("Bearer {}", data_json["AccessToken"].as_str().unwrap()).as_str(),
            )
            .unwrap(),
        );

        headers
    }

    async fn get_symbol_info_internal(&mut self, exchange: &str, symbol: &str, reload: bool) -> Option<Value> {
        if self.symbols.get(format!("{}-{}", exchange, symbol).as_str()).is_none() || reload {
            self.get_symbol(exchange, symbol, None, "Simple").await.unwrap();
        }

        self.symbols.get(format!("{}-{}", exchange, symbol).as_str()).cloned()
    }

    async fn find_symbol_in_exchange(&mut self, symbol: &str) -> Option<String> {
        for exchange in self.exchanges.clone().iter() {
            let si = self.get_symbol_info_internal(exchange, symbol, false).await; // Получаем информацию о тикере

            if let Some(data) = si {
                return Some(data.clone()["board"].as_str()?.to_string());
            }
        }

        None
    }

    async fn find_exchange_by_symbol(&mut self, board: &str, symbol: &str) -> Option<String> {
        for exchange in self.exchanges.clone().iter() {
            let si = self.get_symbol_info_internal(exchange, symbol, false).await; // Получаем информацию о тикере

            if let Some(data) = si {
                if data.clone()["board"] == board {
                    return Some((*exchange.clone()).to_string());
                }
            }
        }

        None
    }

    fn parse_data_from_token(&mut self) {
        let token_data = self.token_data.token_decoded.clone().unwrap();

        let all_agreements = token_data["agreements"]
            .as_str()
            .unwrap()
            .split(" ")
            .collect::<Vec<&str>>();
        let all_portfolios = token_data["portfolios"]
            .as_str()
            .unwrap()
            .split(" ")
            .collect::<Vec<&str>>();

        let mut portfolio_id = 0;
        for (account_id, agreement) in all_agreements.iter().enumerate() {
            // Пробегаемся по всем договорам
            for portfolio in all_portfolios
                .clone()
                .into_iter()
                .skip(portfolio_id)
                .take(3)
            {
                // Пробегаемся по 3 - м портфелям каждого договора
                let (portfolio_type, exchanges, boards) = match portfolio.chars().next() {
                    // Паттерн-матчинг для первых символов
                    Some('D') => (
                        "securities",
                        self.exchanges.clone(),
                        vec![
                            "TQRD".to_string(),
                            "TQOY".to_string(),
                            "TQIF".to_string(),
                            "TQBR".to_string(),
                            "MTQR".to_string(),
                            "TQOB".to_string(),
                            "TQIR".to_string(),
                            "EQRP_INFO".to_string(),
                            "TQTF".to_string(),
                            "FQDE".to_string(),
                            "INDX".to_string(),
                            "TQOD".to_string(),
                            "FQBR".to_string(),
                            "TQCB".to_string(),
                            "TQPI".to_string(),
                            "TQBD".to_string(),
                        ],
                    ),
                    Some('G') => (
                        "fx",
                        vec![self.exchanges[0].clone()],
                        vec![
                            "CETS_SU".to_string(),
                            "INDXC".to_string(),
                            "CETS".to_string(),
                        ],
                    ),
                    Some('7') if portfolio.starts_with("750") => (
                        "derivatives",
                        vec![self.exchanges[0].clone()],
                        vec![
                            "SPBOPT".to_string(),
                            "OPTCOMDTY".to_string(),
                            "OPTSPOT".to_string(),
                            "SPBFUT".to_string(),
                            "OPTCURNCY".to_string(),
                            "RFUD".to_string(),
                            "ROPD".to_string(),
                        ],
                    ),
                    _ => {
                        warn!(
                            "Не определен тип счета для договора {}, портфеля {}",
                            agreement, portfolio
                        );
                        continue;
                    }
                };

                let user = Account {
                    account_id: account_id as i32,
                    agreement: agreement.to_string(),
                    portfolio: portfolio.to_string(),
                    portfolio_type: portfolio_type.to_string(),
                    exchanges,
                    boards,
                };

                self.user_data.accounts.push(user); // Добавляем договор/портфель/биржи/режимы торгов
            }

            portfolio_id += 3; // Смещаем на начальную позицию портфелей для следующего договора
        }
    }

    pub async fn get_history_data_chunk(
        exchange: &str, symbol: &str, tf: i64, from: i64, to: i64, untraded: bool, format: &str,
        block_size: i64, headers: HeaderMap, client: reqwest::Client, url: Url,
    ) -> HistoryDataResponse {
        let response = client
            .get(url)
            .headers(headers)
            .query(&vec![
                ("exchange", exchange),
                ("symbol", symbol),
                ("tf", tf.to_string().as_str()),
                ("from", from.max(to - block_size).to_string().as_str()),
                ("to", to.to_string().as_str()),
                ("untraded", untraded.to_string().as_str()),
                ("format", format),
            ])
            .send()
            .await.unwrap();

        if response.status().is_success() {
            response.json::<HistoryDataResponse>().await.unwrap()
        } else {
            panic!("{:?}", response.text().await.unwrap());
        }
    }

    // api

    pub async fn get_positions(&mut self, portfolio: &str, exchange: &str, without_currency: bool, format: &str) -> Result<Value, Box<dyn StdError>> {
        debug!("start get_positions");

        let response = self
            .client
            .client
            .get(
                self.client
                    .api_server
                    .join(format!("/md/v2/Clients/{}/{}/positions", exchange, portfolio).as_str())
                    .unwrap(),
            )
            .headers(self.get_headers_vec().await)
            .query(&vec![
                ("withoutCurrency", without_currency.to_string().as_str()),
                ("format", format),
            ])
            .send()
            .await;
        let response_json = self.client.validate_response(response.unwrap()).await?;

        Ok(response_json)
    }

    /// get_history(exchange: str, symbol: str, tf: int, seconds_from: int = 1, seconds_to: int = 32536799999, untraded: bool = false, format: str = "Simple", block_size: int = 2500000)
    /// --
    /// Получение свечей за определенный интервал
    ///
    /// :param str exchange: Биржа 'MOEX' или 'SPBX'
    /// :param str symbol: Тикер
    /// :param int tf: длительность таймфрейма
    /// :param int seconds_from: дата с которой нужно получить свечи
    /// :param int seconds_to: дата по которую нужно получить свечи
    /// :param bool untraded:
    /// :param str format: Формат принимаемых данных 'Simple', 'Slim', 'Heavy'
    /// :param int block_size: Размер одного запроса на биржу
    ///
    pub async fn get_history(
        &mut self, exchange: &str, symbol: &str, tf: i64, seconds_from: i64, seconds_to: i64, untraded: bool, format: &str, block_size: i64
    ) -> Result<Value, Box<dyn StdError>> {
        debug!("start get_history");

        let exchange_string = exchange.to_string();
        let symbol_string = symbol.to_string();
        let format_string = format.to_string();
        //let client = reqwest::Client::new();
        let client = reqwest::Client::builder()
            .use_rustls_tls()      // тот же принцип
            .build()
            .unwrap();
        let url = self.client.api_server.join("/md/v2/history").unwrap();
        let headers = self.get_headers_vec().await;

        let (tx, rx) = mpsc::channel();

        let mut result_data = HistoryDataResponse {
            history: vec![],
            next: None,
            prev: None,
        };

        let from: i64 = seconds_from;
        let to: i64 = seconds_to;

        let correct_from: i64;
        let mut correct_to: i64;
        if from + block_size >= to {
            correct_from = from;
            correct_to = to;
        } else {
            correct_from = AlorRust::get_history_data_chunk(
                exchange_string.as_str(), symbol_string.as_str(), tf, from,from + 10, untraded, format_string.as_str(), block_size, headers.clone(),client.clone(), url.clone(),
            ).await.next.unwrap();

            correct_to = AlorRust::get_history_data_chunk(
                exchange_string.as_str(), symbol_string.as_str(), tf, to - 10, to, untraded, format_string.as_str(), block_size, headers.clone(), client.clone(), url.clone(),
            ).await.prev.unwrap();
        }

        let mut tasks: Vec<JoinHandle<Result<HistoryDataResponse, Error>>> = Vec::new();
        loop {
            let exchange_string_copy = exchange_string.clone();
            let symbol_string_copy = symbol_string.clone();
            let format_string_copy = format_string.clone();
            let cloned_client = client.clone();
            let cloned_url = url.clone();
            let cloned_headers = headers.clone();

            tasks.push(tokio::spawn(async move {
                let data = AlorRust::get_history_data_chunk(
                    &exchange_string_copy, &symbol_string_copy, tf, correct_from, correct_to, untraded, &format_string_copy, block_size, cloned_headers.clone(), cloned_client.clone(), cloned_url.clone(),
                ).await;

                Ok(data)
            }));

            correct_to = correct_to - block_size;
            if correct_to <= 0 || correct_to < correct_from {
                break;
            }
        }

        for task in tasks {
            let mut result = task.await.unwrap().unwrap();

            result_data.history.append(&mut result.history);
        }
        result_data.remove_duplicate_histories();
        tx.send(result_data).unwrap();

        let result_data = rx.recv().unwrap();

        Ok(serde_json::to_value(result_data)?)
    }

    /// get_symbol(exchange, symbol, instrument_group=None, format="Simple")
    /// --
    /// Получение информации о выбранном финансовом инструменте
    ///
    /// :param str exchange: Биржа 'MOEX' или 'SPBX'
    /// :param str symbol: Тикер
    /// :param str instrument_group: Код режима торгов
    /// :param str format: Формат принимаемых данных 'Simple', 'Slim', 'Heavy'
    ///
    pub async fn get_symbol(&mut self, exchange: &str, symbol: &str, instrument_group: Option<String>, format: &str) -> Result<Value, Box<dyn StdError>> {
        debug!("start get symbol");
        let mut params = vec![("format", format)];

        if let Some(ref value) = instrument_group {
            params.push(("instrumentGroup", value.as_str()));
        };

        let response = self
            .client
            .client
            .get(self.client
                .api_server
                .join(format!("/md/v2/Securities/{}/{}", exchange, symbol).as_str())
                .unwrap(),
            )
            .headers(self.get_headers_vec().await)
            .query(&params.clone())
            .send()
            .await;
        let mut response_json = self.client.validate_response(response.unwrap()).await?;

        let decimals = ((1.0 / response_json["minstep"].as_f64().unwrap()).log10() + 0.99) as i64;
        response_json["decimals"] = Value::Number(Number::from(decimals));

        self.symbols
            .insert(format!("{}-{}", exchange, symbol), response_json.clone());

        Ok(response_json)
    }

    pub async fn get_symbol_info(&mut self, exchange: &str, symbol: &str, reload: bool) -> Result<Option<Value>, Box<dyn StdError>> {
        debug!("start get symbol info");

        if self.symbols.get(format!("{}-{}", exchange, symbol).as_str()).is_none() || reload {
            self.get_symbol(exchange, symbol, None, "Simple").await?;
        }


        Ok(self.get_symbol_info_internal(exchange, symbol, reload).await)
    }

    pub async fn get_server_time(&mut self) -> Result<u64, Box<dyn StdError>> {
        debug!("start get server time");

        let response = self
            .client
            .client
            .get(self.client
                     .api_server
                     .join("/md/v2/time")
                     .unwrap(),
            )
            .headers(self.get_headers_vec().await)
            .send()
            .await;
        let response_json = self.client.validate_response(response.unwrap()).await?;

        Ok(response_json.as_u64().unwrap())
    }

    // auth

    pub async fn get_jwt_token(&mut self) -> Result<String, Box<dyn StdError>> {
        let now = Utc::now().timestamp();

        if (self.token_data.token.is_none())
            || (now - self.token_data.token_issued > self.token_data.token_ttl as i64)
        {
            let request_url = self.client.oauth_server.join("/refresh")?;

            debug!("Sending RefreshToken: {}", self.token_data.refresh_token);
            let response = self
                .client
                .client
                .post(request_url)
                .query(&vec![("token", self.token_data.refresh_token.clone())])
                .send()
                .await?;

            if response.status().is_success() {
                // todo: remove raw_response and add validate response;
                let response_text = response.text().await?;
                let response_json: Value = serde_json::from_str(&*response_text)?;
                let token = response_json["AccessToken"].as_str().unwrap().to_string();
                self.token_data.raw_response = Some(response_text);
                self.token_data.token = Some(token.clone());
                self.token_data.token_decoded = Some(self.client.decode_token(token)?);
                self.token_data.token_issued = now;

                // todo: move to init?
                self.parse_data_from_token();
            } else {
                self.token_data.token = None;
                self.token_data.token_decoded = None;
                self.token_data.token_issued = 0;
            }
        }

        Ok(self.token_data.raw_response.clone().unwrap())
    }

    // websocket methods

    /// subscribe_bars(self)
    /// --
    /// Подписка на историю цен (свечи) для выбранных биржи и финансового инструмента
    ///
    /// :param str exchange: Биржа 'MOEX' или 'SPBX'
    /// :param str symbol: Тикер
    /// :param tf: Длительность временнОго интервала в секундах или код ("D" - дни, "W" - недели, "M" - месяцы, "Y" - годы)
    /// :param int seconds_from: Дата и время UTC в секундах для первого запрашиваемого бара
    /// :param bool skip_history: Флаг отсеивания исторических данных: True — отображать только новые данные, False — отображать в том числе данные из истории
    /// :param int frequency: Максимальная частота отдачи данных сервером в миллисекундах
    /// :param str format: Формат принимаемых данных 'Simple', 'Slim', 'Heavy'
    /// :return: Уникальный идентификатор подписки
    pub async fn subscribe_bars(
        &mut self, exchange: &str, symbol: &str, tf: u32, seconds_from: u64, skip_history: bool, frequency: i32, format: &str
    ) -> Result<Uuid, Box<dyn StdError>> {
        let data_json: Value = serde_json::from_str(self.get_jwt_token().await?.as_str()).unwrap();
        let auth_token = data_json["AccessToken"].as_str().unwrap().to_string();

        let mut request_body = Value::Object(serde_json::Map::new());

        let subscribe_guid = Uuid::new_v4();
        if let Some(body) = request_body.as_object_mut() {
            body.insert("opcode".to_string(), Value::from("BarsGetAndSubscribe"));
            body.insert("exchange".to_string(), Value::from(exchange));
            body.insert("code".to_string(), Value::from(symbol));
            body.insert("tf".to_string(), Value::Number(Number::from(tf)));
            body.insert("from".to_string(), Value::Number(Number::from(seconds_from)));
            body.insert("delayed".to_string(), Value::from(false)); // Convert false to Value
            body.insert("skipHistory".to_string(), Value::from(skip_history));
            body.insert("frequency".to_string(), Value::from(frequency));
            body.insert("format".to_string(), Value::from(format));
            body.insert("prev".to_string(), Value::from(None::<i32>));
            body.insert("token".to_string(), Value::from(auth_token));
            body.insert("guid".to_string(), Value::from(subscribe_guid.to_string()));
        }

        let message = tokio_tungstenite::tungstenite::Message::Text(Utf8Bytes::from(request_body.to_string()));
        let _result = self.client.socket_client.write_stream.send(message).await;

        Ok(subscribe_guid)
    }

    // convert

    pub async fn dataname_to_board_symbol(&mut self, dataname: &str) -> Result<Vec<String>, Box<dyn StdError>> {
        debug!("parse dataname_to_board_symbol");
        let symbol_parts: Vec<&str> = dataname.split('.').collect();

        let mut board: String;
        let symbol: String;

        if symbol_parts.len() >= 2 {
            // Если тикер задан в формате <Код режима торгов>.<Код тикера>
            board = symbol_parts[0].to_string(); // Код режима торгов
            symbol = symbol_parts[1..].join("."); // Код тикера
        } else {
            symbol = dataname.to_string();
            board = self.find_symbol_in_exchange(symbol.as_str()).await.unwrap().to_string();
        }

        if board.clone() == "SPBFUT" {
            // Для фьючерсов
            board = "RFUD".to_string(); // Меняем код режима торгов на принятое в Алоре
        } else if board.clone() == "SPBOPT" {
            // Для опционов
            board = "ROPD".to_string(); // Меняем код режима торгов на принятое в Алоре
        }

        Ok(vec![board, symbol])
    }

    pub async fn get_exchange(&mut self, board: &str, symbol: &str) -> Result<String, Box<dyn StdError>> {
        debug!("get exchange by board: {board} and symbol: {symbol}");
        let mut using_board = board.to_string();

        if using_board == "SPBFUT" {
            // Для фьючерсов
            using_board = "RFUD".to_string(); // Меняем код режима торгов на принятое в Алоре
        } else if using_board == "SPBOPT" {
            // Для опционов
            using_board = "ROPD".to_string(); // Меняем код режима торгов на принятое в Алоре
        }
        let exchange = self.find_exchange_by_symbol(using_board.as_str(), symbol);

        Ok(exchange.await.unwrap())
    }

    pub fn utc_timestamp_to_msk_datetime(&self, timestamp: i64) -> Result<DateTime<Utc>, Box<dyn StdError>> {
        // Convert Unix timestamp to Utc DateTime
        Ok(DateTime::from_timestamp(timestamp, 0).unwrap())
    }


    /// msk_datetime_to_utc_timestamp(date)
    /// --
    /// Перевод московского времени в кол-во секунд, прошедших с 01.01.1970 00:00 UTC
    ///
    /// :param datetime dt: Московское время
    /// :return: Кол-во секунд, прошедших с 01.01.1970 00:00 UTC
    ///
    pub fn msk_datetime_to_utc_timestamp(&self, date: DateTime<Utc>) -> Result<u64, Box<dyn StdError>> {
        let date_with_tz: DateTime<Tz> = date.with_timezone(&"Europe/Moscow".parse::<Tz>()?);

        Ok(date_with_tz.timestamp() as u64)
    }

    // class methods

    pub fn get_account(&mut self, board: &str, account_id: i32) -> Result<Option<Value>, Box<dyn StdError>> {
        let account = self.user_data.find_account(account_id, board);

        if let Some(account) = account {
            return Ok(Some(serde_json::to_value(account)?));
        };

        Ok(None)
    }

    pub fn accounts(&self) -> Result<Vec<Value>, Box<dyn StdError>> {
        let mut result = vec![];

        for account in self.user_data.accounts.iter() {
            let _ = result.push(serde_json::to_value(account).unwrap());
        }

        Ok(result)
    }
}

pub fn init_logger(level: &str) {
    std::env::set_var("RUST_LOG", level);
    env_logger::init();
}