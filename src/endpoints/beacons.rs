use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{
        broadcast,
        mpsc::{self, error::SendError},
        Mutex,
    },
    task::JoinHandle,
    time::sleep,
};

use super::EndpointModule;

pub const BEACONS_BASE_ENDPOINT: &str = "/beacons";

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
struct BatteryStatus {
    charging: bool,
    percent: u8,
}

#[derive(Debug, Serialize, Clone)]
struct BeaconData {
    description: Option<String>,
    battery: BatteryStatus,
}

#[derive(Debug, Clone)]
struct BeaconState {
    data: Arc<Mutex<HashMap<usize, BeaconData>>>,
    broadcast: broadcast::Sender<WebWebSocketInOutWrapper>,
}

pub struct BeaconModule;

impl EndpointModule for BeaconModule {
    fn create_router() -> axum::Router {
        let state = BeaconState {
            data: Default::default(),
            broadcast: broadcast::channel(16).0,
        };

        Router::new()
            .route("/data", get(data))
            .route("/web", get(web))
            .route("/list", get(list))
            .with_state(state)
    }
}

async fn list(State(state): State<BeaconState>) -> impl IntoResponse {
    #[derive(Debug, Serialize)]
    struct Send {
        data: HashMap<usize, BeaconData>,
    }

    serde_json::to_string(&Send {
        data: state.data.lock().await.clone(),
    })
    .expect("valid serialization")
}

async fn web(State(state): State<BeaconState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn data(State(state): State<BeaconState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_data_socket(state, socket))
}

#[derive(Debug, Deserialize, Serialize, Clone)]
enum WebWebSocketInOut {
    Update {
        message: String,
    },
    Ping,
    /// Only out but I don't want separate enums
    BatteryUpdate {
        status: BatteryStatus,
    },
}

impl WebWebSocketInOut {
    fn wrapping_target(self, target: usize) -> WebWebSocketInOutWrapper {
        WebWebSocketInOutWrapper { target, data: self }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WebWebSocketInOutWrapper {
    target: usize,
    data: WebWebSocketInOut,
}

#[derive(Debug, Serialize)]
enum DataWebSocketOut {
    Update { message: String },
    Ping,
}

#[derive(Debug, Deserialize)]
enum DataWebSocketIn {
    Hello { id: usize, status: BatteryStatus },
    Battery { status: BatteryStatus },
}

async fn handle_data_socket(state: BeaconState, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    let (send_tx, mut send_rx) = mpsc::channel(10);
    let mut brx = state.broadcast.subscribe();
    let btx = state.broadcast.clone();

    let Some(Ok(msg)) = rx.next().await else {
        return;
    };

    let id = match msg {
        Message::Text(txt) => {
            let Ok(msg) = serde_json::from_str::<DataWebSocketIn>(&txt) else {
                return;
            };

            if let DataWebSocketIn::Hello { id, status } = msg {
                state.data.lock().await.insert(
                    id,
                    BeaconData {
                        description: None,
                        battery: status,
                    },
                );

                id
            } else {
                eprintln!("Invalid message! must say hello!");
                return;
            }
        }
        _ => {
            eprintln!("Invalid message! Must say hello!");
            return;
        }
    };

    let receiver: JoinHandle<Result<(), serde_json::Error>> = tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            match msg {
                Message::Close(_) => {
                    state.data.lock().await.remove(&id);
                    break;
                }
                Message::Text(txt) => {
                    let msg: DataWebSocketIn = serde_json::from_str(&txt)?;

                    match msg {
                        DataWebSocketIn::Hello { .. } => eprintln!("Can't send hello again!"),
                        DataWebSocketIn::Battery { status } => {
                            let _ = btx.send(
                                WebWebSocketInOut::BatteryUpdate { status }.wrapping_target(id),
                            );
                        }
                    }
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

    let br_send = send_tx.clone();
    let broadcast_rx = tokio::spawn(async move {
        loop {
            if let Ok(ev) = brx.recv().await {
                if ev.target != id {
                    continue;
                }

                let ev = match ev.data {
                    WebWebSocketInOut::Ping => DataWebSocketOut::Ping,
                    WebWebSocketInOut::Update { message } => DataWebSocketOut::Update { message },
                    _ => continue,
                };

                let _ = br_send
                    .send(Message::Text(
                        serde_json::to_string(&ev).expect("valid serialize"),
                    ))
                    .await;
            }
        }
    });

    tokio::select! {
        _ = broadcast_rx => {},
        _ = receiver => {},
        _ = sender => {},
        _ = ping => {}
    }
}

async fn handle_socket(state: BeaconState, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    let (send_tx, mut send_rx) = mpsc::channel(10);
    let mut brx = state.broadcast.subscribe();
    let btx = state.broadcast.clone();

    let receiver: JoinHandle<Result<(), serde_json::Error>> = tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            match msg {
                Message::Close(_) => {
                    break;
                }
                Message::Text(txt) => {
                    let msg: WebWebSocketInOutWrapper = serde_json::from_str(&txt)?;

                    if matches!(msg.data, WebWebSocketInOut::BatteryUpdate { .. }) {
                        eprintln!("Web clients cannot send battery updates!");
                        continue;
                    }

                    let _ = btx.send(msg);
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

    let br_send = send_tx.clone();
    let broadcast_rx = tokio::spawn(async move {
        loop {
            if let Ok(ev) = brx.recv().await {
                let _ = br_send
                    .send(Message::Text(
                        serde_json::to_string(&ev).expect("valid serialize"),
                    ))
                    .await;
            }
        }
    });

    tokio::select! {
        _ = broadcast_rx => {},
        _ = receiver => {},
        _ = sender => {},
        _ = ping => {}
    }
}
