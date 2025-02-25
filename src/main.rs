use anyhow::{Context, Result, bail};
use container::{Container, ContainerHandle};
use game::{Game, GameResult};
use log::{debug, error, info, warn};
use std::time::Duration;
use std::{env, fs};
use submission::Submission;
use tokio::task::JoinSet;
use tokio::time::timeout;

mod container;
mod db;
mod game;
mod game_map;
mod port_utils;
mod submission;
use db::Database;

const CONTAINER_TIMEOUT: Duration = Duration::from_secs(10);
const GAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Tournament runner for RPLCS HTTP submissions
///
/// To enable logging, set the RUST_LOG environment variable:
/// - PowerShell: `$env:RUST_LOG = "info"; cargo run`
/// - Bash/Shell: `RUST_LOG=info cargo run`
///
/// Available log levels: error, warn, info, debug, trace
#[tokio::main(flavor = "multi_thread", worker_threads = 12)]
async fn main() -> Result<()> {
    env_logger::init();
    info!("Starting the tournament runner");

    let submissions_dir = "submissions";
    let submission_names =
        load_submission_names(submissions_dir).context("Failed to load submissions")?;
    info!(
        "Found {} submissions: {:?}",
        submission_names.len(),
        submission_names
    );

    // TODO: making this shorter for testing
    // let round_robin_pairs = vec![(submission_names[0].clone(), submission_names[1].clone())];
    let round_robin_pairs = round_robin::generate_rounds(submission_names);
    info!(
        "Generated {} matchups for round-robin tournament",
        round_robin_pairs.len()
    );

    let db = Database::new()?;

    for (submission_a, submission_b) in round_robin_pairs {
        info!("Starting matchup: {} vs {}", submission_a, submission_b);
        debug!("Initializing containers for both submissions");

        let container_results = tokio::join!(
            async {
                timeout(CONTAINER_TIMEOUT, Container::new(&submission_a))
                    .await
                    .context("Container A startup timed out")?
                    .context("Failed to create container for {submission_a}")
            },
            async {
                timeout(CONTAINER_TIMEOUT, Container::new(&submission_b))
                    .await
                    .context("Container B startup timed out")?
                    .context("Failed to create container for {submission_b}")
            }
        );

        let (container_a, container_b) = match container_results {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                error!("Failed to initialize containers: {}", e);
                continue;
            }
        };

        let _result = {
            let handle_a = container_a.handle();
            let handle_b = container_b.handle();

            run_games(
                submission_a.clone(),
                submission_b.clone(),
                handle_a,
                handle_b,
                &db,
            )
            .await
        };

        // Shutdown containers
        let (shutdown_a, shutdown_b) = tokio::join!(container_a.shutdown(), container_b.shutdown());

        if let Err(e) = shutdown_a {
            warn!("Failed to shutdown container {}: {}", submission_a, e);
        }
        if let Err(e) = shutdown_b {
            warn!("Failed to shutdown container {}: {}", submission_b, e);
        }
    }

    info!("Tournament completed successfully");
    Ok(())
}

fn load_submission_names(submissions_dir: &str) -> Result<Vec<String>> {
    debug!("Loading submissions from directory: {}", submissions_dir);
    let entries = fs::read_dir(submissions_dir).context("Failed to read submissions directory")?;
    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.context("Failed to read entry")?;
        if entry.path().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }

    info!("Successfully loaded {} submissions", names.len());
    Ok(names)
}

async fn run_games(
    submission_a: String,
    submission_b: String,
    container_a: ContainerHandle,
    container_b: ContainerHandle,
    db: &Database,
) -> Result<Vec<GameResult>> {
    let matchup_id = db.start_matchup(&submission_a, &submission_b).await?;
    let rounds_per_pair = get_rounds_per_pair();

    let mut tasks = JoinSet::new();
    for game_number in 0..rounds_per_pair {
        let is_reversed = game_number % 2 != 0;
        let effective_game_number = if is_reversed {
            rounds_per_pair + game_number
        } else {
            game_number
        };

        let (first_sub, second_sub, first_container, second_container) = if !is_reversed {
            (
                submission_a.clone(),
                submission_b.clone(),
                container_a.clone(),
                container_b.clone(),
            )
        } else {
            (
                submission_b.clone(),
                submission_a.clone(),
                container_b.clone(),
                container_a.clone(),
            )
        };

        tasks.spawn(run_game(
            effective_game_number,
            first_sub,
            second_sub,
            first_container,
            second_container,
            matchup_id,
            db.clone(),
        ));
    }

    let mut results = Vec::with_capacity(rounds_per_pair as usize);
    while let Some(result) = tasks.join_next().await {
        match result
            .context("Failed to join task")?
            .context("Failed to run game")
        {
            Ok(res) => results.push(res),
            Err(e) => {
                for error in e.chain() {
                    warn!("Error: {}", error);
                }
                continue;
            }
        }
    }

    Ok(results)
}

async fn run_game(
    game_id: i64,
    first_submission: String,
    second_submission: String,
    first_container: ContainerHandle,
    second_container: ContainerHandle,
    matchup_id: i64,
    db: Database,
) -> Result<GameResult> {
    debug!(
        "Starting game {} between {} and {}",
        game_id, first_submission, second_submission
    );

    let game_future = async {
        let first = Submission::new(first_submission.as_str(), first_container);
        let second = Submission::new(second_submission.as_str(), second_container);

        let mut game = Game::new(first, second, game_id, matchup_id);
        game.result(&db).await.context("Failed to run game")
    };

    match timeout(GAME_TIMEOUT, game_future).await {
        Ok(result) => {
            let result = result.context("Failed to get game result")?;
            info!(
                "Game {} completed: {} vs {} - {:?}",
                game_id, first_submission, second_submission, result
            );
            Ok(result)
        }
        Err(_) => {
            error!("Game {} timed out after {:?}", game_id, GAME_TIMEOUT);
            bail!("Game timed out")
        }
    }
}

fn get_rounds_per_pair() -> i64 {
    env::var("ROUNDS_PER_PAIR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
}

fn get_turns_per_game() -> i64 {
    env::var("TURNS_PER_GAME")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
}
