use axum::{extract::Query, routing::get, Json, Router};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Map, Value};
use urlencoding::encode;

use super::EndpointModule;

pub const EVENTS_BASE_ENDPOINT: &str = "/events";

#[derive(Deserialize)]
struct GroqParameters {
    groq: String,
}

pub struct EventsModule {}

impl EndpointModule for EventsModule {
    fn create_router() -> Router {
        Router::new().route("/", get(events))
    }
}

async fn events(groq: Query<GroqParameters>) -> Result<Json<serde_json::Value>, StatusCode> {
    let client = Client::new();

    let Ok(response) = client
        .get(format!(
            "https://{}.apicdn.sanity.io/v2023-02-16/data/query/production?query={}",
            std::env::var("SANITY_PROJECT_ID").unwrap(),
            encode(&groq.groq)
        ))
        .bearer_auth(std::env::var("SANITY_TOKEN").unwrap())
        .send()
        .await
    else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let Ok(text) = response.text().await else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let Ok(response_json) = serde_json::from_str::<Map<String, Value>>(&text) else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    if let Some(result) = response_json.get("result") {
        Ok(Json(result.clone()))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
