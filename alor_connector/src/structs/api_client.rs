use base64::engine::general_purpose;
use base64::Engine;
use reqwest::Url;
use serde_json::Value;
use std::error::Error;
use std::str::FromStr;
use tokio_tungstenite::tungstenite::http::Uri;
use crate::structs::websocket_state::WebSocketState;

// #[derive(Clone)]
pub struct ApiClient {
    pub client: reqwest::Client,
    pub oauth_server: Url,
    pub api_server: Url,
    // pub cws_server: Url,
    pub socket_client: WebSocketState
}

impl ApiClient {
    pub async fn new(demo: bool, get_bar_callback: fn(&Value)) -> Result<Self, Box<dyn Error>> {
        let oauth_server: Url;
        let api_server: Url;
        // let cws_server: Url;
        let ws_server: Uri;
        if demo {
            oauth_server = Url::parse("https://oauthdev.alor.ru")?;
            api_server = Url::parse("https://apidev.alor.ru")?;
            // cws_server = Url::parse("wss://apidev.alor.ru/cws")?;
            ws_server = Uri::from_str("wss://apidev.alor.ru/ws")?;
        } else {
            oauth_server = Url::parse("https://oauth.alor.ru")?;
            api_server = Url::parse("https://api.alor.ru")?;
            // cws_server = Url::parse("wss://api.alor.ru/cws")?;
            ws_server = Uri::from_str("wss://api.alor.ru/ws")?;
        }

        Ok(ApiClient {
            client: reqwest::Client::builder()
                .use_rustls_tls()          // ← TLS через rustls
                .build()
                .unwrap(),
            oauth_server,
            api_server,
            socket_client: WebSocketState::new(ws_server, get_bar_callback).await,
        })
    }
}

impl ApiClient {
    pub async fn validate_response(
        &self,
        response: reqwest::Response,
    ) -> Result<Value, Box<dyn Error>> {
        let response_json: Value = serde_json::from_str(&response.error_for_status()?.text().await?)?;

        Ok(response_json)
    }

    pub fn decode_token(&self, token: String) -> Result<Value, Box<dyn Error>> {
        let token_split = token.split(".");

        let token_data = token_split.skip(1).next().unwrap();
        let data_string = Self::decode_base64_with_padding(token_data)?;

        Ok(serde_json::from_str(&data_string)?)
    }

    fn decode_base64_with_padding(encoded: &str) -> Result<String, Box<dyn Error>> {
        // Calculate the number of padding characters needed
        let padding_needed = (4 - encoded.len() % 4) % 4;

        // Create a new string with padding
        let padded_encoded = format!("{}{}", encoded, "=".repeat(padding_needed));

        Ok(String::from_utf8(
            general_purpose::STANDARD.decode(&padded_encoded)?,
        )?)
    }
}
