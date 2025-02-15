use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use log::{debug, info};
use rplcs_events::tournament_1::{
    ChoiceResponse, FightChoices, FightInfo, GambleChoices, MoveChoices,
};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Clone)]
struct AppState {
    choice_counter: std::sync::Arc<AtomicU8>,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let state = AppState {
        choice_counter: std::sync::Arc::new(AtomicU8::new(0)),
    };

    let app = Router::new()
        .route("/", get(|| async { () }))
        .route("/health", get(health_check)) // Add health check endpoint
        .route("/choices", post(handle_choices))
        .route("/gamble", post(handle_gamble))
        .route("/fight", post(handle_fight))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// Add health check handler
async fn health_check() -> &'static str {
    "OK"
}

async fn handle_choices(
    State(state): State<AppState>,
    Query(_params): Query<HashMap<String, String>>,
    Json(_choices): Json<MoveChoices>,
) -> Json<ChoiceResponse> {
    debug!("Received choices: {:?}", _choices);
    let current = state.choice_counter.fetch_xor(1, Ordering::SeqCst);
    Json(ChoiceResponse {
        choice_index: current as usize,
    })
}

async fn handle_gamble(Query(_params): Query<HashMap<String, String>>) -> Json<GambleChoices> {
    debug!("Received gamble request");
    Json(GambleChoices::Health)
}

async fn handle_fight(
    Query(_params): Query<HashMap<String, String>>,
    Json(_fight_info): Json<FightInfo>,
) -> Json<FightChoices> {
    debug!("Received fight request");
    Json(FightChoices::Fight)
}
