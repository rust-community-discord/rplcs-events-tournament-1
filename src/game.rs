use crate::{TURNS_PER_GAME, db::Database, game_map::GameMap, submission::Submission};
use anyhow::{Context, Result, anyhow};
use log::{debug, info};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use rand::{prelude::*, random, rngs::StdRng};
use rplcs_events::tournament_1::{
    FightChoices, FightInfo, GambleChoices, MapNodeType, MoveChoices, PlayerState,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum GameResult {
    Player1Win,
    Player2Win,
    Tie,
}

pub struct Game {
    players: [Submission; 2],
    player_positions: [NodeIndex; 2],
    enemies: [PlayerState; 2],
    enemy_positions: [NodeIndex; 2],
    map: GameMap,
    rng: StdRng,
    seed: i64,
    game_id: i64,
    matchup_id: i64,
}

#[derive(Debug, Clone)]
pub struct WrappedChoices {
    pub node_types: Vec<MapNodeType>,
    internal_choices: Vec<NodeIndex>,
}

#[derive(Debug, Clone, Copy)]
enum FightTarget {
    Opponent,
    Enemy(usize),
}

impl Game {
    pub fn new(player_a: Submission, player_b: Submission, game_id: i64, matchup_id: i64) -> Self {
        info!(
            "Creating game {} between {} and {}",
            game_id,
            player_a.name(),
            player_b.name()
        );

        // Generate random seed
        let seed = random::<i64>();
        let mut rng = StdRng::seed_from_u64(seed as u64);
        let map = GameMap::new(&mut rng).expect("Failed to generate map");

        let player_a_position = map
            .get_random_empty_node(&[], &mut rng)
            .expect("No nodes in map");
        let player_b_position = map
            .get_random_empty_node(&[player_a_position], &mut rng)
            .expect("No nodes in map");

        let mut game = Game {
            players: [player_a, player_b],
            player_positions: [player_a_position, player_b_position],
            enemies: [PlayerState::default(), PlayerState::default()],
            enemy_positions: [NodeIndex::new(0), NodeIndex::new(0)],
            map,
            rng,
            seed,
            game_id,
            matchup_id,
        };

        // Initialize both enemies
        game.generate_enemy(0);
        game.generate_enemy(1);

        game
    }

    pub async fn result(&mut self, db: &Database) -> Result<GameResult> {
        info!(
            "Starting game {} between {} and {}",
            self.game_id,
            self.players[0].name(),
            self.players[1].name()
        );

        // Create game record with seed before starting turns
        let game_db_id = db
            .create_game(self.matchup_id, self.game_id, self.seed)
            .await?;

        // Get consistent folder names using the cache
        let (first_name, second_name) = db
            .get_matchup_order(self.players[0].name(), self.players[1].name())
            .await;

        for current_turn in 0..TURNS_PER_GAME {
            // First, save the current state as SVG
            let svg_path = PathBuf::from(format!(
                "results/visualizations/{}_vs_{}/game_{}/turn_{}.svg",
                first_name, second_name, self.game_id, current_turn
            ));

            // Render current state
            self.map.render_to_file(
                self.player_positions,
                self.enemy_positions,
                &[
                    self.players[0].player_state().clone(),
                    self.players[1].player_state().clone(),
                ],
                &self.enemies,
                &svg_path,
            )?;

            // Record the turn in database
            db.record_turn(game_db_id, current_turn, svg_path.to_str().unwrap())
                .await?;

            let player = (current_turn % 2) as usize;
            debug!(
                "Game {} Turn {}: Player {}'s turn",
                self.game_id,
                current_turn,
                self.players[player].name()
            );

            let choices = self.get_available_moves(player);
            let response = self.players[player]
                .get_choices(
                    &MoveChoices {
                        choices: choices.node_types,
                    },
                    self.game_id,
                )
                .await
                .context("result()")?;

            if let Some(&node_to) = choices.internal_choices.get(response.choice_index) {
                self.handle_player_movement(player, self.player_positions[player], node_to)
                    .await
                    .context("result()")?;
            } else {
                // Invalid choice, damage player and skip turn
                self.damage_player(player);
            }

            if let Some(result) = self.check_game_over() {
                info!(
                    "Game {} ended early on turn {}: {:?}",
                    self.game_id, current_turn, result
                );
                // Update game result
                db.update_game_result(self.matchup_id, game_db_id, result)
                    .await?;
                return Ok(result);
            }

            self.handle_enemy_turn().await.context("result()")?;

            if let Some(result) = self.check_game_over() {
                info!(
                    "Game {} ended early on turn {}: {:?}",
                    self.game_id, current_turn, result
                );
                // Update game result
                db.update_game_result(self.matchup_id, game_db_id, result)
                    .await?;
                return Ok(result);
            }
        }

        info!(
            "Game {} ended in tie after {} turns",
            self.game_id, TURNS_PER_GAME
        );
        let result = GameResult::Tie;
        db.update_game_result(self.matchup_id, game_db_id, result)
            .await?;
        Ok(result)
    }

    fn check_game_over(&self) -> Option<GameResult> {
        if self.players.iter().any(|p| p.player_state().health <= 0) {
            if self.players[0].player_state().health <= 0 {
                Some(GameResult::Player2Win)
            } else if self.players[1].player_state().health <= 0 {
                Some(GameResult::Player1Win)
            } else {
                None
            }
        } else {
            None
        }
    }

    async fn handle_fight(&mut self, player: usize, target: FightTarget) -> Result<bool> {
        let player_power = self.players[player].player_state().power;
        let player_name = self.players[player].name();

        let (enemy_power, target_name) = match target {
            FightTarget::Opponent => {
                let other_player = 1 - player;
                (
                    self.players[other_player].player_state().power,
                    self.players[other_player].name().to_string(),
                )
            }
            FightTarget::Enemy(enemy_idx) => (
                self.enemies[enemy_idx].power,
                format!("Enemy {}", enemy_idx),
            ),
        };

        debug!(
            "Game {} Fight: {} ({} power) vs {} ({} power)",
            self.game_id, player_name, player_power, target_name, enemy_power
        );

        let player_wins = self
            .rng
            .random_ratio(player_power, player_power + enemy_power);

        if player_wins {
            debug!(
                "Game {} Player {} wins fight against {}",
                self.game_id, player_name, target_name
            );
            match target {
                FightTarget::Opponent => {
                    let other_player = 1 - player;
                    self.damage_player(other_player);
                    self.player_positions[other_player] =
                        self.get_random_empty_node().context("handle_fight()")?;
                }
                FightTarget::Enemy(enemy_idx) => {
                    let power_gain = enemy_power / 2;
                    self.players[player].player_state_mut().power += power_gain;
                    self.generate_enemy(enemy_idx);
                }
            }
        } else {
            debug!(
                "Game {} Player {} loses fight against {}",
                self.game_id, player_name, target_name
            );
            self.damage_player(player);
            self.player_positions[player] =
                self.get_random_empty_node().context("handle_fight()")?;
        }

        Ok(player_wins)
    }

    async fn handle_gamble(&mut self, player: usize) -> Result<()> {
        let player_name = self.players[player].name().to_string();
        debug!("Game {} Player {} gambling", self.game_id, player_name);

        let response = self.players[player].get_gamble_choice(self.game_id).await?;
        let roll = self.rng.random::<f64>();
        let player_state = self.players[player].player_state_mut();

        let value = match response {
            GambleChoices::Power => &mut player_state.power,
            GambleChoices::Health => &mut player_state.health,
            GambleChoices::Skip => {
                debug!(
                    "Game {} Player {} skipped gambling",
                    self.game_id, player_name
                );
                return Ok(());
            }
        };

        match roll {
            x if x < 0.1 => *value /= 2,           // 10% chance to halve
            x if x < 0.2 => *value *= 2,           // 10% chance to double
            x if x < 0.6 => *value += 1,           // 40% chance to gain 1
            _ => *value = value.saturating_sub(1), // 40% chance to lose 1
        }

        // cap health if it was gambled
        if matches!(response, GambleChoices::Health) {
            player_state.health = player_state.health.min(player_state.max_health);
        }

        match response {
            GambleChoices::Power => {
                let old_power = player_state.power;
                debug!(
                    "Game {} Player {} power gamble: {} -> {}",
                    self.game_id, player_name, old_power, player_state.power
                );
            }
            GambleChoices::Health => {
                let old_health = player_state.health;
                debug!(
                    "Game {} Player {} health gamble: {} -> {}",
                    self.game_id, player_name, old_health, player_state.health
                );
            }
            _ => {}
        }

        Ok(())
    }

    fn heal_player(&mut self, player: usize) {
        let (old_health, new_health) = {
            let player_state = self.players[player].player_state_mut();
            let old_health = player_state.health;
            player_state.health = (player_state.health + 1).min(player_state.max_health);
            (old_health, player_state.health)
        };
        debug!(
            "Game {} Player {} healed: {} -> {} health",
            self.game_id,
            self.players[player].name(),
            old_health,
            new_health
        );
    }

    fn damage_player(&mut self, player: usize) {
        let (old_health, new_health) = {
            let player_state = self.players[player].player_state_mut();
            let old_health = player_state.health;
            player_state.health = player_state.health.saturating_sub(1);
            (old_health, player_state.health)
        };
        debug!(
            "Game {} Player {} took damage: {} -> {} health",
            self.game_id,
            self.players[player].name(),
            old_health,
            new_health
        );
    }

    async fn handle_enemy_turn(&mut self) -> Result<()> {
        for i in 0..self.enemies.len() {
            let current_pos = self.enemy_positions[i];
            let blocked = [self.enemy_positions[0], self.enemy_positions[1]];

            let moves = self
                .map
                .shuffle_available_moves(current_pos, &blocked, &mut self.rng);

            if let Some(new_pos) = moves.first() {
                self.enemy_positions[i] = *new_pos;

                // Check if landed on player
                let player_positions = self.player_positions.clone();
                for (player_idx, &player_pos) in player_positions.iter().enumerate() {
                    if player_pos == *new_pos {
                        self.handle_fight(player_idx, FightTarget::Enemy(i))
                            .await
                            .context("handle_enemy_turn()")?;
                    }
                }
            }
        }
        Ok(())
    }

    fn check_for_fights(&self, player: usize) -> Option<FightTarget> {
        let node = self.player_positions[player];
        let other_player = 1 - player;

        // Check for opponent first
        if self.player_positions[other_player] == node {
            return Some(FightTarget::Opponent);
        }

        // Check for enemies
        if let Some(enemy_idx) = self.enemy_positions.iter().position(|&pos| pos == node) {
            return Some(FightTarget::Enemy(enemy_idx));
        }

        None
    }

    fn get_random_empty_node(&mut self) -> Result<NodeIndex> {
        let blocked = [
            self.player_positions[0],
            self.player_positions[1],
            self.enemy_positions[0],
            self.enemy_positions[1],
        ];
        self.map
            .get_random_empty_node(&blocked, &mut self.rng)
            .ok_or(anyhow!("get_random_empty_node: No empty nodes"))
    }

    // Helper method to create/recreate a single enemy
    fn generate_enemy(&mut self, index: usize) {
        self.enemies[index] = PlayerState {
            health: 1,
            max_health: 1,
            power: self.rng.random_range(2..=7),
        };
        self.enemy_positions[index] = self.get_random_empty_node().unwrap_or_default();
    }

    fn get_available_moves(&mut self, player: usize) -> WrappedChoices {
        let current_pos = self.player_positions[player];
        let targets = self.map.get_outgoing_edges(current_pos);

        let mut node_types = Vec::new();
        let mut internal_choices = Vec::new();

        for target in targets {
            let target = target.target();
            if let Some(node_type) = self.map.get_node_type(target) {
                node_types.push(node_type);
                internal_choices.push(target);
            }
        }

        WrappedChoices {
            node_types,
            internal_choices,
        }
    }

    async fn handle_node_effect(&mut self, player: usize, node_type: MapNodeType) -> Result<bool> {
        match node_type {
            MapNodeType::Healing => {
                self.heal_player(player);
                Ok(false)
            }
            MapNodeType::Gamble => {
                self.handle_gamble(player).await?;
                Ok(false)
            }
            MapNodeType::Teleport => {
                self.player_positions[player] = self
                    .get_random_empty_node()
                    .context("handle_node_effect()")?;
                Ok(true)
            }
            MapNodeType::Normal => Ok(false),
        }
    }

    async fn handle_combat_encounter(
        &mut self,
        player: usize,
        fight_target: FightTarget,
    ) -> Result<()> {
        match fight_target {
            FightTarget::Opponent => {
                self.handle_fight(player, fight_target).await?;
                Ok(())
            }
            FightTarget::Enemy(enemy) => {
                let fight_info = FightInfo::Enemy(self.enemies[enemy]);
                let response = self.players[player]
                    .get_fight_choice(&fight_info, self.game_id)
                    .await
                    .context("handle_combat_encounter()")?;

                match response {
                    FightChoices::Fight => {
                        self.handle_fight(player, fight_target)
                            .await
                            .context("handle_combat_encounter()")?;
                        Ok(())
                    }
                    FightChoices::Flee => self
                        .handle_flee(player)
                        .await
                        .context("handle_combat_encounter()"),
                }
            }
        }
    }

    async fn handle_flee(&mut self, player: usize) -> Result<()> {
        let current_pos = self.player_positions[player];
        let blocked = [
            self.player_positions[0],
            self.player_positions[1],
            self.enemy_positions[0],
            self.enemy_positions[1],
        ];

        let moves = self
            .map
            .shuffle_available_moves(current_pos, &blocked, &mut self.rng);
        let new_pos = moves.first().copied().unwrap_or_else(|| {
            self.get_random_empty_node()
                .context("handle_flee()")
                .unwrap_or_default()
        });

        self.handle_escape_move(player, current_pos, new_pos)
            .await
            .context("handle_flee()")
    }

    async fn handle_regular_move(
        &mut self,
        player: usize,
        _node_from: NodeIndex,
        node_to: NodeIndex,
    ) -> Result<()> {
        self.player_positions[player] = node_to;

        // Handle node effects first
        if let Some(node_type) = self.map.get_node_type(node_to) {
            if self
                .handle_node_effect(player, node_type)
                .await
                .context("handle_regular_move()")?
            {
                // If true was returned, this was a teleport
                let new_pos = self
                    .get_random_empty_node()
                    .context("handle_regular_move()")?;
                return self
                    .handle_escape_move(player, node_to, new_pos)
                    .await
                    .context("handle_regular_move()");
            }
        }

        // Then check for fights
        if let Some(fight_target) = self.check_for_fights(player) {
            return self
                .handle_combat_encounter(player, fight_target)
                .await
                .context("handle_regular_move()");
        }

        Ok(())
    }

    async fn handle_escape_move(
        &mut self,
        player: usize,
        _node_from: NodeIndex,
        node_to: NodeIndex,
    ) -> Result<()> {
        self.player_positions[player] = node_to;

        // Handle node effects, but ignore teleport results since we don't chain escapes
        if let Some(node_type) = self.map.get_node_type(node_to) {
            if node_type != MapNodeType::Teleport {
                let _ = self
                    .handle_node_effect(player, node_type)
                    .await
                    .context("handle_escape_move()")?;
            }
        }

        Ok(())
    }

    async fn handle_player_movement(
        &mut self,
        player: usize,
        node_from: NodeIndex,
        node_to: NodeIndex,
    ) -> Result<()> {
        debug!(
            "Game {} Player {} moving from {:?} to {:?}",
            self.game_id,
            self.players[player].name(),
            node_from,
            node_to
        );
        self.handle_regular_move(player, node_from, node_to)
            .await
            .context("handle_player_movement()")
    }
}
