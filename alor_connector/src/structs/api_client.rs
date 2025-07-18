// alor_connector/src/structs/api_client.rs

use base64::engine::general_purpose;
use base64::Engine;
use reqwest::Url;
use serde_json::Value;
use std::error::Error;
use std::str::FromStr;

// alias the `http::Uri` crate so that `http` can remain your variable name
use http::Uri as HttpUri;
use tokio_tungstenite::tungstenite::http::Uri;

use tokio::sync::watch;

use crate::structs::websocket_state::WebSocketState;
use alor_http::AlorHttp;



pub struct ApiClient {
    pub client: reqwest::Client,
    pub oauth_server: Url,
    pub api_server: Url,
    pub socket_client: WebSocketState,
}

impl ApiClient {
    pub async fn new(demo: bool) -> Result<Self, Box<dyn Error>> {
        // 1) Pick endpoints
        let (oauth_server, api_server, ws_server): (Url, Url, HttpUri) = if demo {
            (
                Url::parse("https://oauthdev.alor.ru")?,
                Url::parse("https://apidev.alor.ru")?,
                "wss://apidev.alor.ru/ws".parse()?,
            )
        } else {
            (
                Url::parse("https://oauth.alor.ru")?,
                Url::parse("https://api.alor.ru")?,
                "wss://api.alor.ru/ws".parse()?,
            )
        };

        // 2) Create your HTTP client
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .build()?;

        // 3) Create the AlorHttp (with JWT refresh)
        let refresh_token = std::env::var("REFRESH_TOKEN")
            .expect("REFRESH_TOKEN not set");
        let alor_http = std::sync::Arc::new(
            AlorHttp::new(refresh_token, demo).await?
        );

        // 4) Create a shutdown channel for graceful WS teardown
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        // 5) Spin up your WS pipeline
        let socket_client =
            WebSocketState::new_pipeline(ws_server, alor_http, shutdown_rx)
                .await?;

        Ok(ApiClient {
            client,
            oauth_server,
            api_server,
            socket_client,
        })
    }

    // … your existing validate_response and decode_token methods …
    pub async fn validate_response(
        &self,
        response: reqwest::Response,
    ) -> Result<Value, Box<dyn Error>> {
        let text = response.error_for_status()?.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn decode_token(&self, token: String) -> Result<Value, Box<dyn Error>> {
        let parts: Vec<&str> = token.split('.').collect();
        let payload = parts.get(1)
            .ok_or("malformed JWT")?;
        let decoded = Self::decode_base64_with_padding(payload)?;
        Ok(serde_json::from_str(&decoded)?)
    }

    fn decode_base64_with_padding(
        encoded: &str
    ) -> Result<String, Box<dyn Error>> {
        let pad = (4 - encoded.len() % 4) % 4;
        let s = format!("{}{}", encoded, "=".repeat(pad));
        let bytes = general_purpose::STANDARD.decode(&s)?;
        Ok(String::from_utf8(bytes)?)
    }
}
