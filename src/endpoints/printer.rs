use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch::{self, Sender},
    task::JoinHandle,
};

use super::EndpointModule;

pub const PRINTER_BASE_ENDPOINT: &str = "/printer";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UnderlineMode {
    None,
    Single,
    Double,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum JustifyMode {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum PrinterInstruction {
    Text(String),
    Image(String),
    Reverse(bool),
    Underline(UnderlineMode),
    Justify(JustifyMode),
    Strike(bool),
    Bold(bool),
    Italic(bool),
    PrintCut,
}

pub type PrinterMessage = Vec<PrinterInstruction>;

#[derive(Clone)]
struct PrinterState {
    sender: Sender<PrinterMessage>,
}

#[derive(Clone)]
enum SocketSenderMessage {
    Print(PrinterMessage),
    PingPong(),
}

pub struct PrinterModule {}

impl EndpointModule for PrinterModule {
    fn create_router() -> Router {
        let (sender, _) = watch::channel::<PrinterMessage>(PrinterMessage::new());

        let state = PrinterState { sender };

        Router::new()
            .route("/", get(live))
            .route("/print", post(print))
            .with_state(state)
    }
}

async fn live(State(state): State<PrinterState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: PrinterState, mut socket: WebSocket) {
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

    let mut job_reciever = state.sender.subscribe();

    let (send_task_sender, mut send_task_receiver) =
        watch::channel::<SocketSenderMessage>(SocketSenderMessage::PingPong());

    let send_task_sender_2 = send_task_sender.clone();

    let mut send_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (send_task_receiver.changed().await).is_ok() {
                let next_message = send_task_receiver.borrow_and_update().clone();

                match next_message {
                    SocketSenderMessage::Print(job) => {
                        let Ok(job_string) = serde_json::to_string(&job) else {
                            continue;
                        };

                        if sender.send(Message::Text(job_string)).await.is_err() {
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

    let mut job_watcher_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (job_reciever.changed().await).is_ok() {
                let _ = send_task_sender.send(SocketSenderMessage::Print(
                    job_reciever.borrow_and_update().clone(),
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
        }
    });

    tokio::select! {
        _rv_a = (&mut send_task) => {
            job_watcher_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        },
        _rv_b = (&mut recv_task) => {
            send_task.abort();
            job_watcher_task.abort();
            ping_pong_task.abort();
        }
        _rv_c = (&mut job_watcher_task) => {
            send_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        }
        _rv_d = (&mut ping_pong_task) => {
            send_task.abort();
            job_watcher_task.abort();
            recv_task.abort();
        }
    }
}

async fn print(
    State(state): State<PrinterState>,
    payload: String,
) -> Result<StatusCode, StatusCode> {
    if !payload.is_empty() {
        let Ok(message): Result<PrinterMessage, serde_json::Error> = serde_json::from_str(&payload)
        else {
            return Err(StatusCode::BAD_REQUEST);
        };

        let _ = state.sender.send(message);

        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}
