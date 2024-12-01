mod endpoints;

use std::net::SocketAddr;

use axum::{routing::get, Json, Router};
use dotenv::dotenv;
use endpoints::{
    doorbell::{DoorbellModule, DOORBELL_BASE_ENDPOINT},
    events::{EventsModule, EVENTS_BASE_ENDPOINT},
    phonebell::{PhoneBellModule, PHONEBELL_BASE_ENDPOINT},
    printer::{PrinterModule, PRINTER_BASE_ENDPOINT},
    EndpointModule,
};
use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    dotenv().ok();

    let app = Router::new()
        .route("/", get(root))
        .nest(DOORBELL_BASE_ENDPOINT, DoorbellModule::create_router())
        .nest(PHONEBELL_BASE_ENDPOINT, PhoneBellModule::create_router())
        .nest(EVENTS_BASE_ENDPOINT, EventsModule::create_router())
        .nest(PRINTER_BASE_ENDPOINT, PrinterModule::create_router());
    // TODO: Add your modules here using this syntax :3

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4226").await.unwrap();

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
