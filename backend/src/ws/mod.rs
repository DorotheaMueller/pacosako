/// Handles all the websocket client logic.
pub mod wake_up_queue;

use crate::{
    actors::websocket::SocketId,
    db,
    login::session,
    protection::SideProtection,
    sync_match::{CurrentMatchState, CurrentMatchStateClient, SynchronizedMatch},
    ServerError,
};
use anyhow::bail;
use axum::extract::ws::Message;
use chrono::{DateTime, Utc};
use log::{info, warn};
use once_cell::sync::OnceCell;
use pacosako::{PacoAction, PlayerColor};
use serde::{Deserialize, Serialize};
use serde_json::de::from_str;
use serde_json::ser::to_string;
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::{Receiver, Sender};

// Everything can send messages to the logic. The logic is a singleton.
pub static TO_LOGIC: OnceCell<Sender<LogicMsg>> = OnceCell::new();

pub async fn to_logic(msg: LogicMsg) {
    if let Err(e) = TO_LOGIC
        .get()
        .expect("Logic not initialized.")
        .send(msg)
        .await
    {
        warn!("Error sending message to logic: {}", e);
    }
}

pub fn run_server(pool: db::Pool) {
    let (to_logic, message_queue) = tokio::sync::mpsc::channel(100);
    TO_LOGIC
        .set(to_logic)
        .expect("Error setting up the TO_LOGIC static variable.");

    wake_up_queue::spawn_sleeper_thread();

    run_logic_server(message_queue, pool);
}

/// A message that is send to the logic where the logic has then to react.
#[derive(Debug)]
pub enum LogicMsg {
    Websocket {
        data: Message,
        source: SocketId,
    },
    Timeout {
        key: String,
        timestamp: DateTime<Utc>,
    },
    AiAction {
        key: String,
        action: PacoAction,
    },
}

/// Spawn a thread that handles the server logic.
fn run_logic_server(message_queue: Receiver<LogicMsg>, pool: db::Pool) {
    std::thread::spawn(move || {
        // Create a runtime that _must_ be driven from a call
        // to `Runtime::block_on`.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // This will run the runtime and future on the current thread
        rt.block_on(loop_logic_server(message_queue, pool))
            .expect("Error running the logic server.");
    });
}

/// Simple loop that reacts to all the messages.
async fn loop_logic_server(
    mut message_queue: Receiver<LogicMsg>,
    pool: db::Pool,
) -> Result<(), ServerError> {
    let mut server_state = ServerState::default();

    while let Some(msg) = message_queue.recv().await {
        let mut conn = pool.0.acquire().await?;
        let res = handle_message(msg, &mut server_state, &mut conn).await;
        if let Err(e) = res {
            warn!("Error in the websocket: {:?}", e);
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct ServerState {
    rooms: HashMap<String, GameRoom>,
}

impl ServerState {
    /// Returns a room, creating it if required. The socket that asked is added
    /// to the room automatically.
    fn room(&mut self, game: &SynchronizedMatch, asked_by: SocketId) -> &mut GameRoom {
        let room = self.rooms.entry(game.key.clone()).or_insert(GameRoom {
            connected: HashSet::new(),
            white_player: SideProtection::for_user(game.white_player),
            black_player: SideProtection::for_user(game.black_player),
        });
        room.connected.insert(asked_by);
        room
    }
    /// Call this method if we determine that a room is not backed by any game
    /// or if the last client disconnects.
    fn destroy_room(&mut self, key: &String) {
        self.rooms.remove(key);
    }
}

#[derive(Debug)]
struct GameRoom {
    connected: HashSet<SocketId>,
    white_player: SideProtection,
    black_player: SideProtection,
}

/// All allowed messages that may be send by the client to the server.
#[derive(Deserialize)]
enum ClientMessage {
    DoAction { key: String, action: PacoAction },
    Rollback { key: String },
    TimeDriftCheck { send: DateTime<Utc> },
}

#[derive(Deserialize)]
struct RoutedClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    data: String,
}

#[derive(Deserialize)]
struct SubscribeToMatchSocketData {
    key: String,
}

/// Messages that may be send by the server to the client.
#[derive(Clone, Serialize, Debug)]
pub enum ServerMessage {
    CurrentMatchState(Box<CurrentMatchStateClient>),
    Error(String),
    TimeDriftResponse {
        send: DateTime<Utc>,
        bounced: DateTime<Utc>,
    },
}

/// This handle message is wired up, so that each message is handled separately.
/// What we likely actually want is to only run messages sequentially that
/// concern the same game key. So that is some possible performance improvement
/// that is still open here.
async fn handle_message(
    msg: LogicMsg,
    server_state: &mut ServerState,
    conn: &mut db::Connection,
) -> Result<(), anyhow::Error> {
    match msg {
        LogicMsg::Websocket { data, source } => {
            info!("Data is: {:?}", data);

            match data {
                Message::Text(ref text) => {
                    if let Ok(client_msg) = from_str(text) {
                        handle_client_message(client_msg, source, server_state, conn).await?;
                    } else if let Ok(client_msg) = from_str(text) {
                        let x: RoutedClientMessage = client_msg;
                        println!(
                            "Routed Client Message of type {} with data {}.",
                            x.message_type, x.data
                        );
                        if x.message_type == "subscribeToMatchSocket" {
                            if let Ok(data) = from_str::<SubscribeToMatchSocketData>(&x.data) {
                                handle_subscribe_to_match(data.key, source, server_state, conn)
                                    .await?;
                            }
                        }
                    }
                }
                Message::Binary(payload) => {
                    warn!("Binary message received: {:?}", payload);
                    bail!("Binary message received: {:?}", payload);
                }
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Close(_) => {
                    info!("Close message received. This is not implemented here.");
                }
            };
            Ok(())
        }
        LogicMsg::Timeout { key, timestamp } => {
            info!("Timeout was called for game {} at {}", key, timestamp);

            let mut game = fetch_game(&key, conn).await?;

            // TODO: Figure out where to load the additional metadata.
            let state = progress_the_timer(&mut game, key.clone()).await?;

            store_game(&game, conn).await?;
            if let Some(room) = server_state.rooms.get_mut(&game.key) {
                let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
                broadcast_state(room, &client_state).await;
            }
            Ok(())
        }
        LogicMsg::AiAction { key, action } => {
            let mut game = fetch_game(&key, conn).await?;

            let state = progress_the_timer(&mut game, key.clone()).await?;

            if state.victory_state.is_over() {
                store_game(&game, conn).await?;
                if let Some(room) = server_state.rooms.get_mut(&key) {
                    let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
                    broadcast_state(room, &client_state).await;
                }
                return Ok(()); // Do not do the AI action if the game is over.
            }

            let state = game.do_action(action)?;
            store_game(&game, conn).await?;
            if let Some(room) = server_state.rooms.get_mut(&key) {
                let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
                broadcast_state(room, &client_state).await;
            }

            Ok(())
        }
    }
}

async fn progress_the_timer(
    game: &mut SynchronizedMatch,
    key: String,
) -> Result<CurrentMatchState, anyhow::Error> {
    match game.timer_progress() {
        Ok(mut state) => {
            if state.victory_state.is_over() {
                return Ok(state);
            } else if let Some(timer) = &mut state.timer {
                let next_reminder = timer.timeout(state.controlling_player);
                wake_up_queue::put_utc(key, next_reminder).await;
            }
            Ok(state)
        }
        Err(e) => {
            bail!("Error when progressing the timer: {}", e);
        }
    }
}

async fn handle_client_message(
    msg: ClientMessage,
    sender: SocketId,
    server_state: &mut ServerState,
    conn: &mut db::Connection,
) -> Result<(), anyhow::Error> {
    match msg {
        ClientMessage::DoAction { key, action } => {
            let game = fetch_game(&key, conn).await;
            let Ok(mut game) = game else {
                server_state.destroy_room(&key);
                send_error(format!("Game {key} not found"), &sender).await;
                return Ok(());
            };

            let room = server_state.room(&game, sender);

            ensure_uuid_is_allowed(room, &mut game, sender, conn).await?;

            let state = progress_the_timer(&mut game, key.clone()).await?;

            if state.victory_state.is_over() {
                store_game(&game, conn).await?;
                let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
                broadcast_state(room, &client_state).await;
                return Ok(());
            }

            let state = game.do_action(action)?;
            store_game(&game, conn).await?;
            let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
            broadcast_state(room, &client_state).await;
        }
        ClientMessage::Rollback { key } => {
            let mut game = fetch_game(&key, conn).await?;
            let room = server_state.room(&game, sender);

            ensure_uuid_is_allowed(room, &mut game, sender, conn).await?;

            if game.actions.is_empty() {
                // If there are no actions yet, rolling back does nothing.
                return Ok(());
            }

            let state = progress_the_timer(&mut game, key.clone()).await?;

            if state.victory_state.is_over() {
                store_game(&game, conn).await?;
                let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
                broadcast_state(room, &client_state).await;
                return Ok(());
            }

            let state = game.rollback()?;
            store_game(&game, conn).await?;
            let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
            broadcast_state(room, &client_state).await;
        }
        ClientMessage::TimeDriftCheck { send } => {
            send_msg(
                ServerMessage::TimeDriftResponse {
                    send,
                    bounced: Utc::now(),
                },
                &sender,
            )
            .await;
        }
    }
    Ok(())
}

async fn handle_subscribe_to_match(
    key: String,
    sender: SocketId,
    server_state: &mut ServerState,
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
) -> Result<(), anyhow::Error> {
    let game = fetch_game(&key, conn).await?;
    let state = game.current_state();
    let Ok(state) = state else {
        server_state.destroy_room(&key);
        send_error(format!("Could not connect to game {key}"), &sender).await;
        return Ok(());
    };

    server_state.room(&game, sender);

    if let Some(ref timer) = state.timer {
        if !timer.get_state().is_finished() {
            let next_reminder = timer.timeout(state.controlling_player);
            wake_up_queue::put_utc(&key, next_reminder).await;
        }
    }
    let client_state = CurrentMatchStateClient::try_new(state, conn).await?;
    let response = ServerMessage::CurrentMatchState(Box::new(client_state));
    send_msg(response, &sender).await;
    Ok(())
}

/// If the game is running in safe mode, this will check if the sender is allowed
/// to perform actions in the game. Or if the current player slot is no assigned
/// yet, then the sender will be assigned to the slot.
///
/// In case of a UserId assigned as side protection, we also update the game
/// to persist this on the database.
///
/// If there are two different players connected, then the first player can only
/// control white while the second player can only control black.
///
/// Please note that currently game protection is not persisted across server
/// restart. This means it is possible that the first move is done by black.
async fn ensure_uuid_is_allowed(
    room: &mut GameRoom,
    game: &mut SynchronizedMatch,
    sender: SocketId,
    conn: &mut db::Connection,
) -> Result<(), anyhow::Error> {
    if !game.setup_options.safe_mode {
        return Ok(());
    }

    let white_is_moving = game.current_state()?.controlling_player == PlayerColor::White;

    let side_protection = if white_is_moving {
        &mut room.white_player
    } else {
        &mut room.black_player
    };

    if let Some((uuid, session_id)) = sender.get_owner() {
        let mut user_id = None;
        if let Some(session_id) = session_id {
            let session_data = session::load_session(session_id, conn).await?;
            user_id = Some(session_data.user_id);
        }

        let is_allowed = side_protection.test_and_assign(&uuid, user_id);

        if let Some(user_id) = side_protection.get_user() {
            if white_is_moving {
                game.white_player = Some(user_id);
            } else {
                game.black_player = Some(user_id);
            }
        }

        if is_allowed {
            return Ok(());
        }
    } else if side_protection.is_unclaimed() {
        return Ok(());
    }
    anyhow::bail!("Your browser is not allowed to make moves for the current player.");
}

async fn broadcast_state(room: &mut GameRoom, state: &CurrentMatchStateClient) {
    let mut offenders = vec![];
    for target in &room.connected {
        // Remove participants that have disconnected.
        if target.is_alive() {
            send_msg(
                ServerMessage::CurrentMatchState(Box::new(state.clone())),
                target,
            )
            .await;
        } else {
            offenders.push(*target);
        }
    }

    for offender in offenders {
        room.connected.remove(&offender);
    }
}

async fn send_msg(message: ServerMessage, target: &SocketId) {
    let Ok(msg) = to_string(&message) else {
        warn!("Could not serialize message: {:?}", message);
        return;
    };
    send_raw_msg(target, Message::Text(msg)).await;
}

async fn send_raw_msg(target: &SocketId, out_msg: Message) {
    target.send(out_msg).await;
}

/// Helper message to make sending error messages easier.
async fn send_error(error_message: String, target: &SocketId) {
    send_msg(ServerMessage::Error(error_message), target).await;
}

async fn fetch_game(
    key: &str,
    conn: &mut db::Connection,
) -> Result<SynchronizedMatch, anyhow::Error> {
    let id = key.parse()?;
    match db::game::select(id, conn).await? {
        Some(game) => Ok(game),
        None => {
            bail!("There is no game with key {}", key)
        }
    }
}

async fn store_game(
    game: &SynchronizedMatch,
    conn: &mut db::Connection,
) -> Result<(), anyhow::Error> {
    db::game::update(game, conn).await?;
    Ok(())
}
