# RPLCS Server Tournament 1

This is the repository for the first server tournament of the [RPLCS Discord
Server](https://discord.gg/rust-lang-community), where players submit programs
in Rust that will get tested automatically by the tournament runner.

## Game Description

The first server tournament of the [RPLCS Discord
Server](https://discord.gg/rust-lang-community) hosts a turn-based strategy game
where two players compete on a graph and battle each other to decide who wins.

### Game Mechanics

- Players move between connected nodes on a randomly generated graph
- Nodes can have either directed or undirected edges to other nodes
- Each node has one of these effects:
  - Normal: No special effect
  - Healing: Restores 1 health point, up to a maximum of 3
  - Gamble: Option to gamble health or power. Players can choose whether they
    want to gamble health or power. Then the selected resource is gambled and it
    has 10% chance to be halved, 10% chance to be doubled, 40% to lose 1, and
    40% chance to gain one.
  - Teleport: Moves player to a random empty node

### Stats and Combat

- Players start with 3 base health and 5 power
- Combat occurs when:
  - A player walks into another player or enemy
  - An enemy walks into a player
- Combat uses the player's power to decide the winner. If player A has 3 power
  and player B has 7 power, then player A has 3 / (3 + 7) = 0.3 = 30% chance to
  win.
- Losing combat results in:
  - Taking damage (1 health point)
  - Being teleported to a random empty node
- Winning combat results in:
  - Gaining half of the defeated enemy's (not player) power
  - Staying in the current node with no change in health

### Victory Conditions

- Eliminate the opponent (reduce their health to 0)
- If no winner after 100 turns, the game ends in a tie

## Tournament Format

- Round-robin tournament where each submission plays against all others
- 50 games per matchup (25 games as first player, 25 as second)
- Results are stored in a SQLite database
- Game states are saved as SVG visualizations
- Final rankings determined by win/loss ratio

## How to Participate

1. Create a HTTP server in Rust that implements the game protocol
2. Server must listen on port 3000
3. Include a Dockerfile in your submission
4. Submit your entry by sharing your GitHub repository in the #tournament_1_submissions channel of the [RPLCS Discord Server](https://discord.gg/rust-lang-community)

## Testing Your Submission

1. Clone this repository
2. Place your submission in the `submissions` folder
3. Use the commands below to test (see command #1 for running the tournament)
4. Check the `results` folder for game visualizations and statistics

## Useful Commands

1. Reset environment and run tournament in debug mode:

```ps
Remove-Item .\results\results.sqlite -ErrorAction SilentlyContinue; $env:RUST_LOG="debug"; cargo run
```

2. Run cargo fmt on all cargo projects:

```ps
Get-ChildItem .\submissions\ -Directory | ForEach-Object { Push-Location $_.FullName; cargo fmt; Pop-Location }; cargo fmt
```

3. Run tests with quickcheck configured:

```ps
$env:QUICKCHECK_TESTS=100000; cargo nextest run
$env:QUICKCHECK_TESTS=100000; cargo test
```

4. Run report generator:

```ps
uv venv
uv pip install pandas matplotlib seaborn
uv run report_generator.py
```

5. Inspect whether or not a submission is running:

```ps
podman inspect -f "{{.State.Running}}" <submission_name>
```

## Game REST API Protocol

Your HTTP server must implement these endpoints to participate in the tournament:

### GET /

Health check endpoint. Should return status 200 OK.

### POST /choices

Receives available moves for the current turn and expects your move choice.

- Request: `MoveChoices` struct containing available node types
- Response: `ChoiceResponse` struct with the index of your chosen move
- Choice index must be valid (within bounds of the available choices array)

### POST /gamble

Called when landing on a gamble node to choose which stat to gamble.

- Request: Empty JSON object `{}`
- Response: `GambleChoices` enum (Power, Health, or Skip)
- Choosing Skip avoids gambling but wastes the opportunity

### POST /fight

Called when encountering an enemy to decide whether to fight or flee.

- Request: `FightInfo` enum with enemy stats
- Response: `FightChoices` enum (Fight or Flee)
- Fleeing teleports you to a random empty node

All data structures are defined in the `rplcs_events` crate under the `tournament_1` module. Request and response bodies use JSON serialization.

Each request includes a `game_id` parameter in the URL query to identify
different game instances between two players.
