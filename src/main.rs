#[macro_use]
extern crate dotenv_codegen;

mod endpoints;

use std::net::SocketAddr;

use axum::{routing::get, Json, Router};
use endpoints::{
    doorbell::{DoorbellModule, DOORBELL_BASE_ENDPOINT},
    events::{EventsModule, EVENTS_BASE_ENDPOINT},
    EndpointModule,
};
use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .nest(DOORBELL_BASE_ENDPOINT, DoorbellModule::create_router())
        .nest(EVENTS_BASE_ENDPOINT, EventsModule::create_router());
    // TODO: Add your modules here using this syntax :3

    #[cfg(debug_assertions)]
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    #[cfg(not(debug_assertions))]
    let listener = tokio::net::TcpListener::bind("0.0.0.0:80").await.unwrap();

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn root() -> Json<Value> {
    Json(json!({ "ok": true, "readme": "Welcome to the Purdue Hackers API!" }))
}
