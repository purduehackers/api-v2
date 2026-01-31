use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::sync::Mutex;

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
    sync::watch::{self},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::config::PHONEBELL_KNOWN_NUMBERS;

use super::EndpointModule;

pub const PHONEBELL_BASE_ENDPOINT: &str = "/phonebell";

#[derive(Clone)]
struct PhoneBellState {
    ringer_state: watch::Sender<bool>,

    call_state_alerter: watch::Sender<()>,
    call_state: Arc<Mutex<HashMap<Uuid, bool>>>,

    webrtc_signaling_message_queue: watch::Sender<WebRTCSignalingRelayMessage>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum PhoneIncomingMessage {
    Dial { number: String },
    Hook { state: bool },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum PhoneOutgoingMessage {
    Ring { state: bool },
    Mute { state: bool },
    PlaySound { sound: Sound },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Sound {
    None,
    Dialtone,
    Ringback,
    Hangup,
}

#[derive(Clone, Debug)]
enum PhoneSocketInternalMessage {
    Ring(bool),
    Mute(bool),
    PlaySound(Sound),
    PingPong,
}

#[derive(PartialEq)]
enum PhoneStatus {
    Idle,
    AwaitingUser,
    CallingOthers,
    InCall,
    AwaitingOthers,
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
        let (call_state_alerter, _) = watch::channel::<()>(());
        let call_state = Arc::new(Mutex::new(HashMap::new()));

        let (ringer_state, _) = watch::channel::<bool>(false);
        let (webrtc_signaling_message_queue, _) =
            watch::channel::<WebRTCSignalingRelayMessage>(WebRTCSignalingRelayMessage::None);

        let state = PhoneBellState {
            ringer_state,

            call_state_alerter,
            call_state,

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

async fn handle_phone_socket(state: PhoneBellState, socket: WebSocket, phone_type: PhoneType) {
    let (mut sender, mut receiver) = socket.split();

    println!("phone waiting for auth...");

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

    println!("phone connected, let's rock and roll");

    let phone_id = Uuid::new_v4();

    let (send_task_sender, mut send_task_receiver) =
        tokio::sync::mpsc::channel::<PhoneSocketInternalMessage>(32);
    let send_task_sender_2 = send_task_sender.clone();
    let send_task_sender_3 = send_task_sender.clone();
    let send_task_sender_4 = send_task_sender.clone();

    let ringer_state_sender = state.ringer_state.clone();
    let ringer_state_sender_2 = state.ringer_state.clone();
    let ringer_state_sender_cleanup = state.ringer_state.clone();
    let mut ringer_state_listener = state.ringer_state.subscribe();

    let mut call_state_alerter = state.call_state_alerter.subscribe();
    let mut call_state_alerter_2 = call_state_alerter.clone();
    let call_state_alerter_cleanup = state.call_state_alerter.clone();
    let call_state = state.call_state.clone();
    let call_state_2 = state.call_state.clone();
    let call_state_3 = state.call_state.clone();

    let phone_status = Arc::new(Mutex::new(PhoneStatus::Idle));
    let phone_status_2 = phone_status.clone();

    let hook_state = Arc::new(Mutex::new(true));
    let hook_state_2 = hook_state.clone();

    (*call_state.lock().await).insert(phone_id, false);
    call_state_alerter.mark_changed();

    let mut send_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if let Some(next_message) = send_task_receiver.recv().await {
                println!("Phone Socket tx: {:?}", next_message);

                match next_message {
                    PhoneSocketInternalMessage::Ring(state) => {
                        let Ok(message_string) =
                            serde_json::to_string(&PhoneOutgoingMessage::Ring { state })
                        else {
                            continue;
                        };

                        if sender.send(Message::Text(message_string)).await.is_err() {
                            return;
                        }
                    }
                    PhoneSocketInternalMessage::Mute(state) => {
                        let Ok(message_string) =
                            serde_json::to_string(&PhoneOutgoingMessage::Mute { state })
                        else {
                            continue;
                        };

                        if sender.send(Message::Text(message_string)).await.is_err() {
                            return;
                        }
                    }
                    PhoneSocketInternalMessage::PlaySound(sound) => {
                        let Ok(message_string) =
                            serde_json::to_string(&PhoneOutgoingMessage::PlaySound { sound })
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

    let mut call_state_watcher_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            tokio::select! {
                ringer_result = ringer_state_listener.changed() => {
                    if ringer_result.is_err() {
                        continue;
                    }
                }
                call_state_result = call_state_alerter_2.changed() => {
                    if call_state_result.is_err() {
                        continue;
                    }
                }
            };

            let new_ringer_state = *ringer_state_listener.borrow_and_update();
            let active_callers = (*call_state_2.lock().await)
                .values()
                .filter(|in_call| **in_call)
                .count();
            let hook_state = *hook_state_2.lock().await;

            let mut locked_phone_status = phone_status_2.lock().await;

            match *locked_phone_status {
                PhoneStatus::Idle => {
                    if hook_state {
                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::Ring(new_ringer_state))
                            .await;
                    }
                }
                PhoneStatus::AwaitingUser => {}
                PhoneStatus::CallingOthers => {
                    if !hook_state && active_callers > 1 {
                        *locked_phone_status = PhoneStatus::InCall;

                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::PlaySound(Sound::None))
                            .await;
                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::Mute(false))
                            .await;
                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::Ring(false))
                            .await;

                        ringer_state_sender.send_modify(|ringer_state| {
                            *ringer_state = false;
                        });
                    }
                }
                PhoneStatus::InCall => {
                    if !hook_state && active_callers == 1 && !new_ringer_state {
                        *locked_phone_status = PhoneStatus::AwaitingOthers;

                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::PlaySound(Sound::Hangup))
                            .await;
                    }
                }
                PhoneStatus::AwaitingOthers => {
                    if !hook_state && active_callers > 1 {
                        *locked_phone_status = PhoneStatus::InCall;

                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::PlaySound(Sound::None))
                            .await;
                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::Mute(false))
                            .await;
                        let _ = send_task_sender_2
                            .send(PhoneSocketInternalMessage::Ring(false))
                            .await;

                        ringer_state_sender.send_modify(|ringer_state| {
                            *ringer_state = false;
                        });
                    }
                }
            };

            drop(locked_phone_status);
        }
    });

    let mut ping_pong_task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            let _ = send_task_sender_3
                .send(PhoneSocketInternalMessage::PingPong)
                .await;

            tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
        }
    });

    let mut recv_task: JoinHandle<()> = tokio::spawn(async move {
        let mut dialed_number = String::from("");

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

            println!("Phone Socket rx: {:?}", message);

            let mut locked_phone_status = phone_status.lock().await;

            match message {
                PhoneIncomingMessage::Dial { number } => {
                    match *locked_phone_status {
                        PhoneStatus::Idle => {
                            dialed_number.push_str(&number);

                            let mut contains = false;

                            for number in PHONEBELL_KNOWN_NUMBERS {
                                if dialed_number == number {
                                    contains = true;
                                }
                            }

                            if !contains {
                                for number in PHONEBELL_KNOWN_NUMBERS {
                                    if number.starts_with(&dialed_number) {
                                        contains = true;
                                    }
                                }

                                if !contains {
                                    dialed_number = String::from("0");
                                }

                                contains = !contains;
                            }

                            if contains {
                                if *hook_state.lock().await {
                                    *locked_phone_status = PhoneStatus::AwaitingUser;

                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Ring(true))
                                        .await;
                                } else {
                                    *locked_phone_status = PhoneStatus::CallingOthers;

                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::PlaySound(
                                            Sound::Ringback,
                                        ))
                                        .await;

                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Mute(false))
                                        .await;

                                    let mut locked_phone_map = call_state.lock().await;

                                    locked_phone_map.insert(phone_id, true);

                                    if locked_phone_map
                                        .values()
                                        .filter(|in_call| **in_call)
                                        .count()
                                        <= 1
                                    {
                                        let _ = ringer_state_sender_2.send(true);
                                    }

                                    drop(locked_phone_map);

                                    call_state_alerter.mark_changed();
                                }
                            }
                        }
                        PhoneStatus::AwaitingUser => {}
                        PhoneStatus::CallingOthers => {}
                        PhoneStatus::InCall => {
                            if phone_type == PhoneType::Inside && number == "0" {
                                // TODO: Tell door opener to open
                            }
                        }
                        PhoneStatus::AwaitingOthers => {}
                    }
                }
                PhoneIncomingMessage::Hook { state } => {
                    *hook_state.lock().await = state;

                    if !state {
                        // User picked up the phone
                        let call_going = (*call_state.lock().await)
                            .values()
                            .filter(|in_call| **in_call)
                            .count()
                            > 0;

                        if call_going {
                            let _ = send_task_sender_4
                                .send(PhoneSocketInternalMessage::Mute(false))
                                .await;
                            let _ = send_task_sender_4
                                .send(PhoneSocketInternalMessage::Ring(false))
                                .await;
                            let _ = send_task_sender_4
                                .send(PhoneSocketInternalMessage::PlaySound(Sound::None))
                                .await;

                            (*call_state.lock().await).insert(phone_id, true);
                            call_state_alerter.mark_changed();

                            let _ = ringer_state_sender_2.send(false);

                            *locked_phone_status = PhoneStatus::InCall;
                        } else {
                            match *locked_phone_status {
                                PhoneStatus::Idle => {
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Ring(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Mute(true))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::PlaySound(
                                            Sound::Dialtone,
                                        ))
                                        .await;
                                }
                                PhoneStatus::AwaitingUser => {
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Ring(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Mute(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::PlaySound(
                                            Sound::Ringback,
                                        ))
                                        .await;

                                    *locked_phone_status = PhoneStatus::CallingOthers;

                                    let mut locked_phone_map = call_state.lock().await;

                                    locked_phone_map.insert(phone_id, true);

                                    if locked_phone_map
                                        .values()
                                        .filter(|in_call| **in_call)
                                        .count()
                                        <= 1
                                    {
                                        let _ = ringer_state_sender_2.send(true);
                                    }

                                    drop(locked_phone_map);

                                    call_state_alerter.mark_changed();
                                }
                                PhoneStatus::CallingOthers => {
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Ring(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Mute(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::PlaySound(
                                            Sound::Ringback,
                                        ))
                                        .await;
                                }
                                PhoneStatus::InCall => {
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Ring(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Mute(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::PlaySound(Sound::None))
                                        .await;
                                }
                                PhoneStatus::AwaitingOthers => {
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Ring(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::Mute(false))
                                        .await;
                                    let _ = send_task_sender_4
                                        .send(PhoneSocketInternalMessage::PlaySound(Sound::Hangup))
                                        .await;
                                }
                            }
                        }
                    } else {
                        // User put down the phone
                        let _ = send_task_sender_4
                            .send(PhoneSocketInternalMessage::PlaySound(Sound::None))
                            .await;
                        let _ = send_task_sender_4
                            .send(PhoneSocketInternalMessage::Mute(true))
                            .await;
                        let _ = send_task_sender_4
                            .send(PhoneSocketInternalMessage::Ring(false))
                            .await;

                        match *locked_phone_status {
                            PhoneStatus::Idle => {
                                dialed_number = String::from("");
                            }
                            PhoneStatus::AwaitingUser => {
                                dialed_number = String::from("");
                            }
                            PhoneStatus::CallingOthers => {
                                (*call_state.lock().await).insert(phone_id, false);
                                call_state_alerter.mark_changed();

                                let _ = ringer_state_sender_2.send(false);
                            }
                            PhoneStatus::InCall => {
                                (*call_state.lock().await).insert(phone_id, false);
                                call_state_alerter.mark_changed();

                                let _ = ringer_state_sender_2.send(false);
                            }
                            PhoneStatus::AwaitingOthers => {
                                (*call_state.lock().await).insert(phone_id, false);
                                call_state_alerter.mark_changed();

                                let _ = ringer_state_sender_2.send(false);
                            }
                        }

                        *locked_phone_status = PhoneStatus::Idle;
                    }
                }
            }

            drop(locked_phone_status);
        }
    });

    tokio::select! {
        _rv_a = (&mut send_task) => {
            call_state_watcher_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        },
        _rv_b = (&mut recv_task) => {
            send_task.abort();
            call_state_watcher_task.abort();
            ping_pong_task.abort();
        }
        _rv_c = (&mut call_state_watcher_task) => {
            send_task.abort();
            ping_pong_task.abort();
            recv_task.abort();
        }
        _rv_d = (&mut ping_pong_task) => {
            send_task.abort();
            call_state_watcher_task.abort();
            recv_task.abort();
        }
    }

    // Clean up on disconnect - remove from call_state and update ringer if needed
    let mut locked_call_state = call_state_3.lock().await;
    let was_in_call = locked_call_state.get(&phone_id).copied().unwrap_or(false);
    locked_call_state.remove(&phone_id);

    // If this phone was in a call, check if anyone else is still calling
    if was_in_call {
        let active_callers = locked_call_state
            .values()
            .filter(|in_call| **in_call)
            .count();

        // If no one else is calling, stop the ringer
        if active_callers == 0 {
            let _ = ringer_state_sender_cleanup.send(false);
        }

        // Notify other phones of the state change
        drop(locked_call_state);
        call_state_alerter_cleanup.send(()).ok();
    }

    println!("Phone disconnected, cleaned up state");
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
    message_queue: watch::Sender<WebRTCSignalingRelayMessage>,
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
