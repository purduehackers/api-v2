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

#[derive(Clone)]
enum SocketSenderMessage {
    Ring(bool),
    PingPong(),
}

pub struct DoorbellModule {}

impl EndpointModule for DoorbellModule {
    fn create_router() -> Router {
        let (sender, receiver) = watch::channel::<bool>(false);

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

    let (send_task_sender, mut send_task_receiver) =
        watch::channel::<SocketSenderMessage>(SocketSenderMessage::PingPong());

    let send_task_sender_2 = send_task_sender.clone();

    let mut send_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (send_task_receiver.changed().await).is_ok() {
                let next_message = send_task_receiver.borrow_and_update().clone();

                match next_message {
                    SocketSenderMessage::Ring(new_state) => {
                        if sender
                            .send(Message::Text(format!("{}", new_state)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    SocketSenderMessage::PingPong() => {
                        if sender
                            .send(Message::Ping(vec![
                                103, 111, 111, 100, 32, 109, 111, 114, 110, 105, 110, 103, 33, 33,
                                33,
                            ]))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        }
    });

    let mut ring_watcher_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (ringing_reciever.changed().await).is_ok() {
                let _ = send_task_sender.send(SocketSenderMessage::Ring(
                    *ringing_reciever.borrow_and_update(),
                ));
            }
        }
    });

    let mut ping_pong_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            let _ = send_task_sender_2.send(SocketSenderMessage::PingPong());

            tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
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
            ring_watcher_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        },
        _rv_b = (&mut recv_task) => {
            send_task.abort();
            ring_watcher_task.abort();
            ping_pong_task.abort();
        }
        _rv_c = (&mut ring_watcher_task) => {
            send_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        }
        _rv_d = (&mut ping_pong_task) => {
            send_task.abort();
            ring_watcher_task.abort();
            recv_task.abort();
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
