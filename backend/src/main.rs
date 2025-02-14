mod caching;
mod config;
mod db;
mod secret_login;
//mod discord;
mod actors;
mod game;
mod language;
mod login;
mod protection;
mod replay_data;
mod server;
mod statistics;
mod sync_match;
mod templates;
#[cfg(test)]
mod test;
mod timer;
mod ws;

#[macro_use]
extern crate log;
extern crate simplelog;
use axum::{
    extract::FromRef,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::EnvironmentConfig;
use db::Pool;
use std::fs::File;

/// This enum holds all errors that can be returned by the API.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Database error")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Could not deserialize data.")]
    DeserializationFailed,
    #[error("Error from the game logic.")]
    GameError(#[from] pacosako::PacoError),
    #[error("(De-)Serialization failed")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Not allowed")]
    NotAllowed,
    #[error("Not found")]
    NotFound,
    #[error("IO-error")]
    IoError(#[from] std::io::Error),
    #[error("Error parsing Integer")]
    ParseIntError(#[from] std::num::ParseIntError),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        match self {
            Self::NotAllowed => (StatusCode::FORBIDDEN, "Not allowed").into_response(),
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Game management /////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

////////////////////////////////////////////////////////////////////////////////
// Set up logging //////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

fn init_logger() {
    use simplelog::{
        ColorChoice, CombinedLogger, Config, LevelFilter, TermLogger, TerminalMode, WriteLogger,
    };

    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Info,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Debug,
            Config::default(),
            File::create("server.log").unwrap(),
        ),
    ])
    .unwrap();

    debug!("Logger successfully initialized");
}

////////////////////////////////////////////////////////////////////////////////
// Start the server ////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

/// Initialize the database Pool and register it as a Rocket state.
pub async fn init_database_pool(config: EnvironmentConfig) -> Pool {
    info!("Creating database pool");
    let now = std::time::Instant::now();

    // If there is no database specified, the server is allowed to just
    // crash. This is why we can "safely" unwrap.

    let pool = db::Pool::new(&config.database_path)
        .await
        .expect("Pool can't be created.");

    // Apply all pending database migrations. (Important for automated updates)
    info!("Starting database migrations (if necessary)");
    let migration_result = sqlx::migrate!().run(&pool.0).await;
    if let Err(migration_error) = migration_result {
        panic!("Migration error when starting the server: {migration_error:?}");
    }
    info!("Database migrated successfully.");
    info!("Pool ready in {}ms", now.elapsed().as_millis());

    pool
}

/// Initialize the websocket server and provide it with a database connection.
fn init_new_websocket_server(pool: Pool) {
    info!("Starting websocket server");
    let now = std::time::Instant::now();

    ws::run_server(pool);

    info!(
        "Websocket server started in {}ms",
        now.elapsed().as_millis()
    );
}

#[derive(Clone)]
pub struct AppState {
    config: EnvironmentConfig,
    pool: Pool,
}

// support converting an `AppState` in an `EnvironmentConfig`
impl FromRef<AppState> for EnvironmentConfig {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.config.clone()
    }
}

// support converting an `AppState` in an `Pool`
impl FromRef<AppState> for Pool {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.pool.clone()
    }
}

#[tokio::main]
async fn main() {
    let config = config::load_config();

    init_logger();

    let pool = init_database_pool(config.clone()).await;

    init_new_websocket_server(pool.clone());

    let state = AppState { config, pool };

    server::run(state).await;
}
