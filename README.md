wrapper for Alor broker
- http
- ws
in order to integrate with rust_bt or barter_rs

cargo build --release
cargo run -p alor_connector --example ws_quotes

commit 1 

– set HTTP-logic to crate alor_http
– AlorHttp::get_history() + AlorHttp::get_history_chunk()
– jwt_refresh_loop()
– keep examples (fetch_history.rs, ws_subscribe.rs)