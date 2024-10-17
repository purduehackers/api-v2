mod endpoints;

use std::net::SocketAddr;

use axum::{
    routing::get,
    Json, Router,
};
use endpoints::{
    doorbell::{DoorbellModule, DOORBELL_BASE_ENDPOINT},
    events::{EventsModule, EVENTS_BASE_ENDPOINT},
    phonebell::{PhoneBellModule, PHONEBELL_BASE_ENDPOINT},
    EndpointModule,
};
use serde_json::{json, Value};
use dotenv::dotenv;

/*
#[derive(Clone, Copy)]
struct Ports {
    http: u16,
    https: u16,
}
*/

#[tokio::main]
async fn main() {
    dotenv().ok();

    let app = Router::new()
        .route("/", get(root))
        .nest(DOORBELL_BASE_ENDPOINT, DoorbellModule::create_router())
        .nest(PHONEBELL_BASE_ENDPOINT, PhoneBellModule::create_router())
        .nest(EVENTS_BASE_ENDPOINT, EventsModule::create_router());
    // TODO: Add your modules here using this syntax :3

    /*
    {
        let ports = Ports {
            http: 4226,

            https: 4227,
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
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    }
    */
    {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:4226").await.unwrap();

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    }
}

async fn root() -> Json<Value> {
    Json(json!({ "ok": true, "readme": "Welcome to the Purdue Hackers API!" }))
}

/*
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
*/
