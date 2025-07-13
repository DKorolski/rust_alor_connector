use alor_http::AlorHttp;
use chrono::{Utc, Duration};
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn compile_only_refresh_test() {
    let server = MockServer::start().await;

    // ── Настраиваем mock эндпоинты ─────────────────────────────
    // 1) успешный ответ на /refresh
    Mock::given(method("POST"))
        .and(path("/refresh"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "AccessToken": "mock_jwt",
                "ExpiresAt":   Utc::now().timestamp() + 1800
            }))
        )
        .mount(&server)
        .await;

    // 2) простой 200 на /md/v2/history
    Mock::given(method("GET"))
        .and(path("/md/v2/history"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": []
            }))
        )
        .mount(&server)
        .await;

    // ── Используем новый конструктор ──────────────────────────
    let http = AlorHttp::new_with_urls(
        "dummy_refresh_token",
        format!("{}/refresh", &server.uri()),
        server.uri(),               // base_url
    )
    .await
    .expect("client should build");

    // один вызов history: проверяем, что возвращается Ok(Vec)
    let _ = http
        .get_history_chunk(
            "IMOEXF",
            "MOEX",
            60,
            Utc::now() - Duration::minutes(1),
            Utc::now(),
        )
        .await
        .expect("history ok");
}
