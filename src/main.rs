#[macro_use]
extern crate dotenv_codegen;

mod endpoints;

use std::{net::SocketAddr, path::PathBuf};

use axum::{
    extract::Host, handler::HandlerWithoutStateExt, http::Uri, response::Redirect, routing::get,
    BoxError, Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use endpoints::{
    doorbell::{DoorbellModule, DOORBELL_BASE_ENDPOINT},
    events::{EventsModule, EVENTS_BASE_ENDPOINT},
    EndpointModule,
};
use reqwest::StatusCode;
use serde_json::{json, Value};

#[derive(Clone, Copy)]
struct Ports {
    http: u16,
    https: u16,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .nest(DOORBELL_BASE_ENDPOINT, DoorbellModule::create_router())
        .nest(EVENTS_BASE_ENDPOINT, EventsModule::create_router());
    // TODO: Add your modules here using this syntax :3

    let ports = Ports {
        #[cfg(debug_assertions)]
        http: 3000,

        #[cfg(not(debug_assertions))]
        http: 80,

        https: 443,
    };

    tokio::spawn(redirect_http_to_https(ports));

    let config = RustlsConfig::from_pem_file(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("certs")
            .join("cert.pem"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("certs")
            .join("key.pem"),
    )
    .await
    .unwrap();

    let addr = SocketAddr::from(([0, 0, 0, 0], ports.https));
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn root() -> Json<Value> {
    Json(json!({ "ok": true, "readme": "Welcome to the Purdue Hackers API!" }))
}

async fn redirect_http_to_https(ports: Ports) {
    fn make_https(host: String, uri: Uri, ports: Ports) -> Result<Uri, BoxError> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&ports.http.to_string(), &ports.https.to_string());
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    }

    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri, ports) {
            Ok(uri) => Ok(Redirect::permanent(&uri.to_string())),
            Err(_) => Err(StatusCode::BAD_REQUEST),
        }
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], ports.http));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
}
