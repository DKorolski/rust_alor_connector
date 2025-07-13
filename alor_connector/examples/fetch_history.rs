// examples/fetch_history.rs
// get_history_data_chunk → вызываем статически, без new()
//        HistoryDataObject {
 //           time: 1752266940,
 //           close: 2642.5,
 //           open: 2646.0,
 //           high: 2646.0,
 //           low: 2642.0,
 //           volume: 830.0,
 //       },
 //   ],
 //   next: None,
 //   prev: Some(
 //       1752258960,
 //   ),
//}

use anyhow::Result;
use chrono::{Duration, Utc};
use reqwest::{Client, header::HeaderMap};
use url::Url;

use alor_connector::AlorRust;
use alor_connector::structs::history_data::HistoryDataResponse;
use alor_http::AlorHttp; 

#[tokio::main]
async fn main() -> Result<()> {
    // Build HTTP headers
    let mut headers = HeaderMap::new();
    let token = std::env::var("REFRESH_TOKEN")
        .expect("REFRESH_TOKEN must be set");
    headers.insert("Authorization", format!("Bearer {}", token).parse()?);
    headers.insert("Accept", "application/json".parse()?);

    // HTTP client & correct URL
    let client = Client::new();
    let url = Url::parse("https://api.alor.ru/md/v2/history")?;

    // Query parameters
    let exchange      = "MOEX";
    let symbol        = "IMOEXF";
    let time_frame    = 60;                // seconds (1-min bars)
    let now           = Utc::now();
    let date_from_sec = (now - Duration::hours(50)).timestamp();
    let date_to_sec   = now.timestamp();
    let untraded      = false;
    let format        = "Simple";
    let block_size    = Duration::hours(1).num_milliseconds();

    // Call the _associated_ function on the type, not on the instance:
    let resp: HistoryDataResponse = AlorRust::get_history_data_chunk(
        exchange,
        symbol,
        time_frame,
        date_from_sec,
        date_to_sec,
        untraded,
        format,
        block_size,
        headers,
        client,
        url,
    )
    .await;

    println!("{:#?}", resp);
    Ok(())
}
