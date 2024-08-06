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
use reqwest::StatusCode;
use serde_json::json;
use tokio::{
    sync::watch::{self, Receiver, Sender},
    task::JoinHandle,
};

use super::EndpointModule;

pub const DOORBELL_BASE_ENDPOINT: &str = "/doorbell";

#[derive(Clone)]
struct DoorbellState {
    sender: Sender<bool>,
    receiver: Receiver<bool>,
}

pub struct DoorbellModule {}

impl EndpointModule for DoorbellModule {
    fn create_router() -> Router {
        let (sender, receiver) = watch::channel(false);

        let state = DoorbellState { sender, receiver };

        Router::new()
            .route("/", get(live))
            .route("/status", get(status))
            .route("/ring", post(ring))
            .with_state(state)
    }
}

async fn live(State(state): State<DoorbellState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: DoorbellState, mut socket: WebSocket) {
    if socket.send(Message::Ping(vec![1, 2, 3])).await.is_err() {
        return;
    }

    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            if let Message::Close(_) = msg {
                return;
            }
        } else {
            return;
        }
    }

    let (mut sender, mut receiver) = socket.split();

    let mut ringing_reciever = state.sender.subscribe();

    let mut send_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (ringing_reciever.changed().await).is_ok()
                && sender
                    .send(Message::Text(format!(
                        "{}",
                        *ringing_reciever.borrow_and_update()
                    )))
                    .await
                    .is_err()
            {
                return;
            }
        }
    });

    let mut recv_task: JoinHandle<()> = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                return;
            }

            if let Message::Text(t) = msg {
                if !t.is_empty() {
                    let new_state = t == "true" || t == "1";

                    let _ = state.sender.send(new_state);
                }
            } else if let Message::Binary(d) = msg {
                if !d.is_empty() {
                    let new_state = d[0] != 0;

                    let _ = state.sender.send(new_state);
                }
            }
        }
    });

    tokio::select! {
        _rv_a = (&mut send_task) => {
            recv_task.abort();
        },
        _rv_b = (&mut recv_task) => {
            send_task.abort();
        }
    }
}

async fn status(State(state): State<DoorbellState>) -> Json<serde_json::Value> {
    Json(json!({ "ringing": *state.receiver.borrow() }))
}

async fn ring(
    State(state): State<DoorbellState>,
    payload: String,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !payload.is_empty() {
        let new_state = payload == "true" || payload == "1";

        Ok(Json(json!({ "ok": state.sender.send(new_state).is_ok() })))
    } else {
        Ok(Json(json!({ "ok": state.sender.send(true).is_ok() })))
    }
}
