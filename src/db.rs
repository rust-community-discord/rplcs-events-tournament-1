use anyhow::{Context, Result};
use log::debug;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use r2d2_sqlite::rusqlite::params;
use rusqlite::OptionalExtension;
use std::time::Duration;
use std::{collections::HashMap, fs, sync::Arc};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::game::GameResult;

#[derive(Clone)]
pub struct Database {
    pool: Pool<SqliteConnectionManager>,
    matchup_cache: Arc<Mutex<HashMap<(String, String), (String, String)>>>,
}

impl Database {
    pub fn new() -> Result<Self> {
        fs::create_dir_all("results")?;
        let manager = SqliteConnectionManager::file("results/results.sqlite");
        let pool = Pool::new(manager).context("Failed to create connection pool")?;

        // Create tables if they don't exist
        let mut conn = pool.get()?;
        let tx = conn.transaction()?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS matchups (
                id INTEGER PRIMARY KEY,
                player_a TEXT NOT NULL,
                player_b TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(player_a, player_b)
            )",
            [],
        )
        .context("Failed to create matchups table")?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS games (
                id INTEGER PRIMARY KEY,
                matchup_id INTEGER NOT NULL,
                game_number INTEGER NOT NULL,
                winner TEXT NOT NULL,
                seed INTEGER NOT NULL,
                FOREIGN KEY(matchup_id) REFERENCES matchups(id),
                UNIQUE(matchup_id, game_number)
            )",
            [],
        )
        .context("Failed to create games table")?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS turns (
                id INTEGER PRIMARY KEY,
                game_id INTEGER NOT NULL,
                turn_number INTEGER NOT NULL,
                svg_path TEXT NOT NULL,
                FOREIGN KEY(game_id) REFERENCES games(id),
                UNIQUE(game_id, turn_number)
            )",
            [],
        )
        .context("Failed to create turns table")?;

        tx.commit()?;

        Ok(Self {
            pool,
            matchup_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn retry_on_locked<F, T>(&self, mut f: F) -> Result<T>
    where
        F: FnMut() -> Result<T>,
    {
        let mut retries = 0;
        let max_retries = 10;
        loop {
            match f() {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if retries >= max_retries {
                        return Err(e);
                    }
                    if let Some(sqlite_error) = e.downcast_ref::<rusqlite::Error>() {
                        if sqlite_error
                            == &rusqlite::Error::SqliteFailure(
                                rusqlite::ffi::Error::new(5), // SQLITE_BUSY
                                Some("database is locked".to_string()),
                            )
                        {
                            let delay = Duration::from_millis(10 * (1 << retries)); // exponential backoff
                            debug!("Database locked, retrying in {:?}", delay);
                            sleep(delay).await;
                            retries += 1;
                            continue;
                        }
                    }
                    return Err(e);
                }
            }
        }
    }

    pub async fn get_matchup_order(&self, player_a: &str, player_b: &str) -> (String, String) {
        let mut cache = self.matchup_cache.lock().await;

        if let Some(order) = cache.get(&(player_a.to_string(), player_b.to_string())) {
            return order.clone();
        }

        // First time seeing this pair, establish the order
        let order = (player_a.to_string(), player_b.to_string());

        // Cache both orderings pointing to the same first ordering
        cache.insert((player_a.to_string(), player_b.to_string()), order.clone());
        cache.insert((player_b.to_string(), player_a.to_string()), order.clone());

        order
    }

    pub async fn start_matchup(&self, player_a: &str, player_b: &str) -> Result<i64> {
        let (first, second) = self.get_matchup_order(player_a, player_b).await;
        let pool = self.pool.clone();
        let first = first.clone();
        let second = second.clone();

        self.retry_on_locked(move || {
            let mut conn = pool.get().context("Failed to get connection from pool")?;

            // Check for existing matchup
            if let Ok(id) = conn.query_row(
                "SELECT id FROM matchups WHERE player_a = ?1 AND player_b = ?2",
                params![first, second],
                |row| row.get::<_, i64>(0),
            ) {
                return Ok(id);
            }

            debug!(
                "INSERT INTO matchups (player_a, player_b) VALUES ({}, {})",
                first, second
            );

            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO matchups (player_a, player_b) VALUES (?1, ?2)",
                params![first, second],
            )
            .context("Failed to insert new matchup")?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    pub async fn create_game(&self, matchup_id: i64, game_number: i64, seed: i64) -> Result<i64> {
        debug!(
            "Creating game: matchup_id={}, game_number={}, seed={}",
            matchup_id, game_number, seed
        );

        let pool = self.pool.clone();
        self.retry_on_locked(move || {
            let mut conn = pool.get().context("Failed to get connection from pool")?;
            let tx = conn.transaction()?;

            let existing_id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM games WHERE matchup_id = ?1 AND game_number = ?2",
                    params![matchup_id, game_number],
                    |row| row.get(0),
                )
                .optional()
                .context("Failed to query existing game")?;

            if let Some(id) = existing_id {
                debug!("Found existing game with id={}", id);
                tx.rollback()?;
                return Ok(id);
            }

            tx.execute(
                "INSERT INTO games (matchup_id, game_number, winner, seed) VALUES (?1, ?2, 'pending', ?3)",
                params![matchup_id, game_number, seed],
            ).context("Failed to insert new game")?;

            let id = tx.last_insert_rowid();
            tx.commit()?;
            debug!("Created new game with id={}", id);
            Ok(id)
        }).await
    }

    pub async fn update_game_result(
        &self,
        matchup_id: i64,
        game_number: i64,
        result: GameResult,
    ) -> Result<()> {
        let winner = match result {
            GameResult::Player1Win => "player_a",
            GameResult::Player2Win => "player_b",
            GameResult::Tie => "tie",
        };
        let pool = self.pool.clone();
        let winner = winner.to_string();

        debug!(
            "UPDATE games SET winner = {} WHERE matchup_id = {} AND game_number = {}",
            winner, matchup_id, game_number
        );

        self.retry_on_locked(move || {
            let mut conn = pool.get().context("Failed to get connection from pool")?;
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE games SET winner = ?1 WHERE matchup_id = ?2 AND game_number = ?3",
                params![winner, matchup_id, game_number],
            )
            .context("Failed to update game result")?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn record_turn(&self, game_id: i64, turn_number: i64, svg_path: &str) -> Result<()> {
        debug!(
            "Recording turn: game_id={}, turn_number={}, svg_path={}",
            game_id, turn_number, svg_path
        );

        let pool = self.pool.clone();
        let svg_path = svg_path.to_string();
        self.retry_on_locked(move || {
            let mut conn = pool.get().context("Failed to get connection from pool")?;
            let tx = conn.transaction()?;

            let exists: bool = tx
                .query_row(
                    "SELECT 1 FROM turns WHERE game_id = ?1 AND turn_number = ?2",
                    params![game_id, turn_number],
                    |_| Ok(true),
                )
                .optional()
                .context("Failed to query existing turn")?
                .unwrap_or(false);

            if exists {
                debug!("Turn already exists, skipping insert");
                tx.rollback()?;
                return Ok(());
            }

            tx.execute(
                "INSERT INTO turns (game_id, turn_number, svg_path) VALUES (?1, ?2, ?3)",
                params![game_id, turn_number, &svg_path],
            )
            .context("Failed to insert new turn")?;

            tx.commit()?;
            debug!("Successfully recorded turn");
            Ok(())
        })
        .await
    }
}
