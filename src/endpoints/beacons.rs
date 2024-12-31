use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{
        mpsc::{self, error::SendError},
        Mutex,
    },
    task::JoinHandle,
    time::sleep,
};

use super::EndpointModule;

pub const BEACONS_BASE_ENDPOINT: &str = "/beacons";

#[derive(Debug)]
struct BatteryStatus {
    charging: bool,
    percent: u8,
}

#[derive(Debug)]
struct BeaconData {
    description: Option<String>,
    battery: BatteryStatus,
}

#[derive(Debug, Default, Clone)]
struct BeaconState {
    data: Arc<Mutex<HashMap<usize, BeaconData>>>,
}

pub struct BeaconModule;

impl EndpointModule for BeaconModule {
    fn create_router() -> axum::Router {
        let state = BeaconState::default();

        Router::new().route("/data", get(live)).with_state(state)
    }
}

async fn live(State(state): State<BeaconState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

#[derive(Debug, Serialize)]
enum WebSocketOut {
    Update { message: String },
    Ping,
}

#[derive(Debug, Deserialize)]
enum WebSocketIn {
    Update { message: String, target: usize },
    Ping { target: usize },
}

async fn handle_socket(state: BeaconState, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    let (send_tx, mut send_rx) = mpsc::channel(10);

    let receiver: JoinHandle<Result<(), serde_json::Error>> = tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            match msg {
                Message::Close(_) => {
                    //
                }
                Message::Text(txt) => {
                    let msg: WebSocketIn = serde_json::from_str(&txt)?;
                }
                _ => {}
            }
        }

        Ok(())
    });

    let sender: JoinHandle<Result<(), String>> = tokio::spawn(async move {
        while let Some(msg) = send_rx.recv().await {
            tx.send(msg).await.map_err(|e| e.to_string())?;
        }

        Ok(())
    });

    let pp_send = send_tx.clone();
    let ping: JoinHandle<Result<(), SendError<Message>>> = tokio::spawn(async move {
        loop {
            pp_send
                .send(Message::Ping(vec![
                    103, 111, 111, 100, 32, 109, 111, 114, 110, 105, 110, 103, 33, 33, 33,
                ]))
                .await?;

            sleep(Duration::from_secs(5)).await;
        }
    });
}
