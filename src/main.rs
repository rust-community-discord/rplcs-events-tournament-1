use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use submission::Submission;

use game::Game;

mod game;
mod submission;
mod wasm_instance_wrapper;

const ROUNDS_PER_PAIR: usize = 50;
const TURNS_PER_GAME: usize = 100;

fn main() -> Result<()> {
    println!("Starting the program...");

    let submissions_dir = "submissions";
    let submissions = load_wasm_programs(submissions_dir)?;
    println!("Loaded WASM programs.");

    let submission_names = submissions.keys().collect::<Vec<_>>();
    let round_robin_pairs = round_robin::generate_rounds(submission_names);

    for (submission_a, submission_b) in round_robin_pairs {
        (0..ROUNDS_PER_PAIR).for_each(|round| {
            let mut game = if round % 2 == 0 {
                let submission_a =
                    Submission::new(submission_a.clone(), &submissions[submission_a])?;
                let submission_b =
                    Submission::new(submission_b.clone(), &submissions[submission_b])?;
                Game::new(submission_a, submission_b)
            } else {
                let submission_a =
                    Submission::new(submission_a.clone(), &submissions[submission_a])?;
                let submission_b =
                    Submission::new(submission_b.clone(), &submissions[submission_b])?;
                Game::new(submission_b, submission_a)
            };

            let result = game.result();

            println!("Result: {:?}", result);
        });
    }

    println!("Program finished.");
    Ok(())
}

fn load_wasm_programs(submissions_dir: &str) -> Result<HashMap<String, Vec<u8>>> {
    let mut submissions = HashMap::new();
    let entries = fs::read_dir(submissions_dir).context("Failed to read submissions directory")?;

    for entry in entries {
        let entry = entry.context("Failed to read submission entry")?;
        let submission_path = entry.path();
        let wasm_dir = submission_path
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("release");

        println!("Checking submission path {}...", submission_path.display());
        println!("Checking WASM dir {}...", wasm_dir.display());

        if wasm_dir.exists() {
            let wasm_files = fs::read_dir(wasm_dir).context("Failed to read target directory")?;

            for wasm_entry in wasm_files {
                let wasm_entry = wasm_entry.context("Failed to read entry")?;
                let path = wasm_entry.path();

                if path.extension().and_then(|ext| ext.to_str()) == Some("wasm") {
                    let submission_name = submission_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    println!("Loading {}...", path.display());
                    let wasm_bytes = fs::read(&path).context("Failed to read WASM file")?;

                    submissions.insert(submission_name, wasm_bytes);
                    break;
                }
            }
        }
    }
    Ok(submissions)
}
