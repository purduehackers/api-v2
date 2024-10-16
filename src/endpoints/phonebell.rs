use std::net::SocketAddr;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    response::{IntoResponse, Response},
    routing::get,
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

pub const PHONEBELL_BASE_ENDPOINT: &str = "/phonebell";

#[derive(Clone)]
struct PhoneBellState {
    ringing_inside: Sender<bool>,

    webrtc_signaling_message_queue: Sender<WebRTCSignalingRelayMessage>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum PhoneIncomingMessage {
    Dial { number: String },
    Hook { state: bool },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum PhoneOutgoingMessage {
    Ring { state: bool },
    ClearDial,
}

#[derive(Clone)]
enum PhoneSocketInternalMessage {
    Ring(bool),
    ClearDial,
    PingPong,
}

#[derive(PartialEq, Clone)]
enum PhoneType {
    Outside,
    Inside,
}

#[derive(Clone)]
enum WebRTCSignalingSocketInternalMessage {
    Relay(WebRTCSignalingRelayMessage),
    PingPong,
}

#[derive(Clone)]
enum WebRTCSignalingRelayMessage {
    Message(String, SocketAddr),
    None,
}

pub struct PhoneBellModule {}

impl EndpointModule for PhoneBellModule {
    fn create_router() -> Router {
        let (ringing_inside, _) = watch::channel::<bool>(false);
        let (webrtc_signaling_message_queue, _) =
            watch::channel::<WebRTCSignalingRelayMessage>(WebRTCSignalingRelayMessage::None);

        let state = PhoneBellState {
            ringing_inside,
            webrtc_signaling_message_queue,
        };

        Router::new()
            .route("/signaling", get(signaling))
            .route("/outside", get(outside))
            .route("/inside", get(inside))
            .with_state(state)
    }
}

async fn outside(
    State(state): State<PhoneBellState>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    Ok(ws.on_upgrade(move |socket| handle_phone_socket(state, socket, PhoneType::Outside)))
}

async fn inside(
    State(state): State<PhoneBellState>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    Ok(ws.on_upgrade(move |socket| handle_phone_socket(state, socket, PhoneType::Inside)))
}

async fn handle_phone_socket(state: PhoneBellState, mut socket: WebSocket, phone_type: PhoneType) {
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

    //Wait for Auth before continuing
    let Some(Ok(msg)) = receiver.next().await else {
        return;
    };

    if let Message::Close(_) = msg {
        return;
    }

    let Message::Text(message_text) = msg else {
        return;
    };

    if message_text != std::env::var("PHONE_API_KEY").unwrap() {
        return;
    }

    let (send_task_sender, mut send_task_receiver) =
        watch::channel::<PhoneSocketInternalMessage>(PhoneSocketInternalMessage::PingPong);
    let send_task_sender_2 = send_task_sender.clone();
    let send_task_sender_3 = send_task_sender.clone();

    let ringing_sender = state.ringing_inside.clone();
    let mut ringing_listener = state.ringing_inside.subscribe();

    let Ok(initial_message_string) = serde_json::to_string(&PhoneOutgoingMessage::Ring {
        state: *ringing_listener.borrow_and_update(),
    }) else {
        return;
    };

    if phone_type == PhoneType::Inside
        && sender
            .send(Message::Text(initial_message_string))
            .await
            .is_err()
    {
        return;
    }

    let mut send_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (send_task_receiver.changed().await).is_ok() {
                let next_message = send_task_receiver.borrow_and_update().clone();

                match next_message {
                    PhoneSocketInternalMessage::Ring(new_state) => {
                        let Ok(message_string) =
                            serde_json::to_string(&PhoneOutgoingMessage::Ring { state: new_state })
                        else {
                            continue;
                        };

                        if sender.send(Message::Text(message_string)).await.is_err() {
                            return;
                        }
                    }
                    PhoneSocketInternalMessage::ClearDial => {
                        let Ok(message_string) =
                            serde_json::to_string(&PhoneOutgoingMessage::ClearDial)
                        else {
                            continue;
                        };

                        if sender.send(Message::Text(message_string)).await.is_err() {
                            return;
                        }
                    }
                    PhoneSocketInternalMessage::PingPong => {
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
    let ring_watcher_phone_type = phone_type.clone();

    let mut ring_watcher_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (ringing_listener.changed().await).is_ok()
                && ring_watcher_phone_type == PhoneType::Inside
            {
                let _ = send_task_sender.send(PhoneSocketInternalMessage::Ring(
                    *ringing_listener.borrow_and_update(),
                ));
            }
        }
    });

    let mut ping_pong_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            let _ = send_task_sender_2.send(PhoneSocketInternalMessage::PingPong);

            tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
        }
    });

    let mut recv_task: JoinHandle<()> = tokio::spawn(async move {
        let mut current_hook_state = true;

        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                return;
            }

            let Message::Text(message_text) = msg else {
                continue;
            };

            let Ok(message): Result<PhoneIncomingMessage, serde_json::Error> =
                serde_json::from_str(&message_text)
            else {
                continue;
            };

            match message {
                PhoneIncomingMessage::Dial { number: _ } => {
                    if phone_type == PhoneType::Inside && !current_hook_state {
                        let _ = send_task_sender_3.send(PhoneSocketInternalMessage::ClearDial);

                        // TODO: Tell door opener to open if number == "0"
                    }
                    // TODO: Do something fun here
                }
                PhoneIncomingMessage::Hook { state } => {
                    current_hook_state = state;

                    if phone_type == PhoneType::Outside && !state {
                        let _ = ringing_sender.send(true);
                    }
                    if phone_type == PhoneType::Inside && !state {
                        let _ = ringing_sender.send(false);
                    }
                    if state {
                        let _ = ringing_sender.send(false);
                    }
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

async fn signaling(
    State(state): State<PhoneBellState>,
    ws: WebSocketUpgrade,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket: WebSocket| {
        handle_signaling_socket(socket, state.webrtc_signaling_message_queue, address)
    })
}

async fn handle_signaling_socket(
    mut socket: WebSocket,
    message_queue: Sender<WebRTCSignalingRelayMessage>,
    my_address: SocketAddr,
) {
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

    let (send_task_sender, mut send_task_receiver) =
        watch::channel::<WebRTCSignalingSocketInternalMessage>(
            WebRTCSignalingSocketInternalMessage::PingPong,
        );
    let send_task_sender_2 = send_task_sender.clone();

    let mut send_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (send_task_receiver.changed().await).is_ok() {
                let next_message = send_task_receiver.borrow_and_update().clone();

                match next_message {
                    WebRTCSignalingSocketInternalMessage::Relay(message) => {
                        let WebRTCSignalingRelayMessage::Message(message_string, origin_address) =
                            message
                        else {
                            continue;
                        };

                        if origin_address != my_address
                            && sender.send(Message::Text(message_string)).await.is_err()
                        {
                            return;
                        }
                    }
                    WebRTCSignalingSocketInternalMessage::PingPong => {
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

    let mut message_queue_listener = message_queue.subscribe();

    let mut webrtc_message_watcher_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if (message_queue_listener.changed().await).is_ok() {
                let _ = send_task_sender.send(WebRTCSignalingSocketInternalMessage::Relay(
                    message_queue_listener.borrow_and_update().clone(),
                ));
            }
        }
    });

    let mut ping_pong_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            let _ = send_task_sender_2.send(WebRTCSignalingSocketInternalMessage::PingPong);

            tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
        }
    });

    let mut recv_task: JoinHandle<()> = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                return;
            }

            let Message::Text(message_text) = msg else {
                continue;
            };

            let _ = message_queue.send(WebRTCSignalingRelayMessage::Message(
                message_text,
                my_address,
            ));
        }
    });

    tokio::select! {
        _rv_a = (&mut send_task) => {
            webrtc_message_watcher_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        },
        _rv_b = (&mut recv_task) => {
            send_task.abort();
            webrtc_message_watcher_task.abort();
            ping_pong_task.abort();
        }
        _rv_c = (&mut webrtc_message_watcher_task) => {
            send_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        }
        _rv_d = (&mut ping_pong_task) => {
            send_task.abort();
            webrtc_message_watcher_task.abort();
            recv_task.abort();
        }
    }
}
