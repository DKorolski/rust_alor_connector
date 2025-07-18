use std::{sync::Arc, time::Duration};
use chrono::{DateTime, Utc};
use tokio::{sync::RwLock, time::sleep};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Ошибки верхнего уровня
#[derive(Debug, Error)]
pub enum AlorError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),         

    #[error("alor api responded with code {code}: {msg}")]
    Api { code: u16, msg: String },

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}


/// Один бар
#[derive(Debug, Clone, Deserialize)]
pub struct Bar {
    pub time:   i64,
    pub open:   f64,
    pub high:   f64,
    pub low:    f64,
    pub close:  f64,
    pub volume: f64,
}

#[derive(Clone)]
pub struct AlorHttp {
    base_url:      String,          // https://api.alor.ru  |  https://api.alor.dev
    refresh_token: String,
    jwt_token:     Arc<RwLock<String>>,
    jwt_expires:   Arc<RwLock<DateTime<Utc>>>,
    client:        Client,
}

#[allow(dead_code)]
const REFRESH_ENDPOINT: &str = "https://oauth.alor.ru/refresh";




impl AlorHttp {
    /// Создаём клиент, получаем первый JWT и запускаем refresh-loop
    pub async fn new(refresh: impl Into<String>, demo: bool) -> Result<Self, AlorError> {
        let base = if demo { "https://api.alor.ru" } else { "https://api.alor.ru" };
        let client = Client::builder()
            .user_agent("alor-http/0.1")
            .timeout(Duration::from_secs(10))
            .build()?;

        let me = Self {
            base_url: base.into(),
            refresh_token: refresh.into(),
            jwt_token: Arc::new(RwLock::new(String::new())),
            jwt_expires: Arc::new(RwLock::new(Utc::now())),
            client,
        };

        me.refresh_jwt().await?;              // сразу получаем токен
        me.spawn_refresh_loop();              // фоновая задача
        Ok(me)
    }

    /// Авто-обновление JWT за 30 с до истечения
    fn spawn_refresh_loop(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                // вычисляем sleep-delay
                let expires = { *this.jwt_expires.read().await };
                let now = Utc::now();
                let delay = expires - chrono::Duration::seconds(30) - now;
                if delay.num_milliseconds() > 0 {
                    sleep(delay.to_std().unwrap()).await;
                }
                // пробуем обновить
                if let Err(e) = this.refresh_jwt().await {
                    eprintln!("JWT refresh failed: {e:?} – retrying in 10 s");
                    sleep(Duration::from_secs(10)).await;
                }
            }
        });
    }

    /// Ходим на /refresh, сохраняем токен и TTL
    async fn refresh_jwt(&self) -> Result<(), AlorError> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "AccessToken")]
            token: String,
            #[serde(rename = "ExpiresAt")]
            expires_at: Option<i64>,   // ← стало Option
        }
        let oauth_url = "https://oauth.alor.ru/refresh";
        let resp: Resp = self.client
            .post(oauth_url)          // prod: https://oauth.alor.ru/refresh
            .json(&serde_json::json!({ "token": self.refresh_token }))
            .send().await?
            .error_for_status()?
            .json().await?;

        let expiry = match resp.expires_at {
            Some(ts) => DateTime::<Utc>::from_utc(
                chrono::NaiveDateTime::from_timestamp_opt(ts, 0).unwrap(),
                Utc),
            None => Utc::now() + chrono::Duration::minutes(25), // ← дефолт
        };

        {
            let mut w = self.jwt_token.write().await;
            *w = resp.token;
        }
        {
            let mut w = self.jwt_expires.write().await;
            *w = expiry;
        }

        Ok(())
    }

    /// Вспомогательный конструктор — только для модульных тестов.
    /// Позволяет подменить oauth-endpoint и base-url на локальный mock.
    #[doc(hidden)]
    pub async fn new_with_urls(
        refresh_token: impl Into<String>,
        oauth_url:     impl Into<String>,
        base_url:      impl Into<String>,
    ) -> Result<Self, AlorError> {
        let client = reqwest::Client::builder()
            .user_agent("alor-http/0.1-test")
            .timeout(Duration::from_secs(5))
            .build()?;

        let this = Self {
            base_url:      base_url.into(),
            refresh_token: refresh_token.into(),
            jwt_token:     Arc::new(RwLock::new(String::new())),
            jwt_expires:   Arc::new(RwLock::new(Utc::now())),
            client,
        };

        // временно переопределяем константный REFRESH_ENDPOINT через поле (!)  
        // (проще всего — сохраняем в self.base_url и добавляем "/refresh" при запросе)
        // Для тестов достаточно прямо вызвать refresh_jwt() с oauth_url
        this.refresh_jwt_custom(oauth_url.into()).await?;
        this.spawn_refresh_loop();
        Ok(this)
    }

    /// Внутренний помощник: обновить JWT, обращаясь к произвольному URL
    #[doc(hidden)]
    async fn refresh_jwt_custom(&self, oauth_url: String) -> Result<(), AlorError> {
        #[derive(Deserialize)]
        struct Resp { #[serde(rename="AccessToken")] token: String,
                      #[serde(rename="ExpiresAt")]  expires_at: i64 }

        let resp: Resp = self.client.post(oauth_url)
            .json(&serde_json::json!({ "token": self.refresh_token }))
            .send().await?.error_for_status()?.json().await?;

        {
            let mut w = self.jwt_token.write().await;
            *w = resp.token;
        }
        {
            let mut w = self.jwt_expires.write().await;
            *w = DateTime::<Utc>::from_utc(
                chrono::NaiveDateTime::from_timestamp_opt(resp.expires_at, 0).unwrap(),
                Utc);
        }
        Ok(())
    }

    pub async fn jwt(&self) -> String {
        self.jwt_token.read().await.clone()
    }

    /// ====== ПУБЛИЧНЫЙ МЕТОД: GET HISTORY (с чанками) ======
    pub async fn get_history(
        &self,
        code: &str,
        exchange: &str,
        tf: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Bar>, AlorError> {
        let mut cursor = from;
        let mut out = Vec::new();
        // Чанки по 30 000 баров макс.  -> секунд в чанке:
        let max_bars = 30_000i64;
        let chunk_sec = tf * max_bars;
        while cursor < to {
            let chunk_to = std::cmp::min(cursor + chrono::Duration::seconds(chunk_sec), to);
            let mut part = self.get_history_chunk(code, exchange, tf, cursor, chunk_to).await?;
            out.append(&mut part);
            cursor = chunk_to + chrono::Duration::seconds(tf);
        }
        Ok(out)
    }

    /// ====== Ассоциированная: один запрос ======
    pub async fn get_history_chunk(
        &self,
        code: &str,
        exchange: &str,
        tf: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Bar>, AlorError> {
        #[derive(Deserialize)]
        struct Response { data: Vec<Bar> }

        let url = Url::parse_with_params(
            &(self.base_url.clone() + "/md/v2/history"),
            &[
                ("exchange", exchange),
                ("code", code),
                ("tf", &tf.to_string()),
                ("from", &from.timestamp().to_string()),
                ("to", &to.timestamp().to_string()),
                ("format", "Simple"),
                ("untraded", "false"),
            ])?;

        let jwt = { self.jwt_token.read().await.clone() };
        let resp = self.client
            .get(url.clone())
            .bearer_auth(&jwt)
            .send().await?;

        if resp.status().as_u16() == 401 {
            // пробуем один refresh и повтор
            self.refresh_jwt().await?;
            let jwt = { self.jwt_token.read().await.clone() };
            return self.client
                .get(url)
                .bearer_auth(&jwt)
                .send().await?
                .error_for_status()?
                .json::<Response>().await
                .map(|r| r.data)
                .map_err(Into::into);
        }

        resp.error_for_status()?
            .json::<Response>().await
            .map(|r| r.data)
            .map_err(Into::into)
    }
}
