use crate::{submission::Submission, TURNS_PER_GAME};
use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
};
use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;
use rplcs_events::tournament_1::{
    ChoiceResponse, FightInfo, GambleChoice, MapNodeType, PlayerChoices, PlayerState,
    PossiblePlayerFightChoices,
};

#[derive(Debug, Clone, Copy)]
pub enum GameResult {
    Player1Win,
    Player2Win,
    Tie,
}

#[derive(Debug, Clone)]
pub enum GameState {
    TurnStart,
    TurnEnd,
    EnemiesTurn,
}

pub struct Game {
    players: [Submission; 2],
    player_positions: [NodeIndex; 2],
    enemies: [PlayerState; 2],
    enemy_positions: [NodeIndex; 2],
    map: DiGraph<MapNode, i32>,
    node_indices: Vec<NodeIndex>,
    event_log: Vec<String>,
    state: GameState,
}

#[derive(Debug, Clone)]
pub struct WrappedChoices {
    pub node_types: Vec<MapNodeType>,
    internal_choices: Vec<NodeIndex>,
}

#[derive(Debug, Clone, Copy)]
pub enum InternalPlayerChoice {
    MoveToNode(NodeIndex, MapNodeType),
    Gamble,
}

#[derive(Debug, Clone, Copy)]
enum FightTarget {
    Opponent,
    Enemy(usize),
}

#[derive(Debug, Clone)]
pub struct MapNode {
    pub node_type: MapNodeType,
    pub enemies: Vec<PlayerState>,
}

impl MapNode {
    pub fn new(node_type: MapNodeType) -> Self {
        MapNode {
            node_type,
            enemies: Vec::new(),
        }
    }
}

impl Game {
    pub fn new(player_a: Submission, player_b: Submission) -> Self {
        let (map, node_indices) = Game::generate_game_map();

        let player_a_position = node_indices.choose(&mut rand::rng()).unwrap();
        let player_b_position = node_indices.choose(&mut rand::rng()).unwrap();

        let mut game = Game {
            players: [player_a, player_b],
            player_positions: [*player_a_position, *player_b_position],
            enemies: [PlayerState::default(), PlayerState::default()],
            enemy_positions: [NodeIndex::new(0), NodeIndex::new(0)],
            map,
            node_indices,
            event_log: Vec::new(),
            state: GameState::TurnStart,
        };

        // Initialize both enemies
        game.generate_enemy(0);
        game.generate_enemy(1);

        game
    }

    pub fn result(&mut self) -> GameResult {
        for current_turn in 0..TURNS_PER_GAME {
            let player = current_turn % 2;

            let choices = self.get_available_moves(player);
            let response = self.players[player].get_choices(&PlayerChoices {
                choices: choices.node_types,
            });

            if let Some(&node_to) = choices.internal_choices.get(response.choice_index) {
                self.handle_player_movement(player, self.player_positions[player], node_to);
            } else {
                // Invalid choice, damage player and skip turn
                self.players[player].player_state_mut().health -= 1;
            }

            // free memory
            self.players[player].submission.call_function_0("free");

            self.handle_enemy_turn();

            // Check for game over conditions
            if self.players.iter().any(|p| p.player_state.health <= 0) {
                if self.players[0].player_state.health <= 0 {
                    return GameResult::Player2Win;
                } else if self.players[1].player_state.health <= 0 {
                    return GameResult::Player1Win;
                }
            }
        }

        GameResult::Tie
    }

    // Helper method to create/recreate a single enemy
    pub fn generate_enemy(&mut self, index: usize) {
        self.enemies[index] = PlayerState {
            health: 1,
            max_health: 1,
            power: rand::random_range(2..=7),
        };
        self.enemy_positions[index] = self.get_random_empty_node()
    }

    fn get_random_empty_node(&self) -> NodeIndex {
        loop {
            let node = self.node_indices.choose(&mut rand::rng()).unwrap();
            let is_teleport_node = self.map.node_weight(*node).map_or(false, |node| {
                matches!(node.node_type, MapNodeType::Teleport)
            });
            let has_player = self.player_positions.contains(node);
            let has_enemy = self.enemy_positions.contains(node);
            if !has_player && !has_enemy && !is_teleport_node {
                return *node;
            }
        }
    }

    /// Handles a fight between a player and either an opponent or enemy.
    /// If the player wins and the target is an opponent, the opponent's health is reduced by 1 and
    /// they are moved to a random empty node.
    /// If the player wins and the target is an enemy, the player gains half of the enemy's power
    /// and a new enemy is created at a random empty node.
    /// If the player loses, their health is reduced by 1 and they are moved to a random empty node.
    /// Returns true if the player wins, false otherwise.
    fn handle_fight(&mut self, player: usize, target: FightTarget) -> bool {
        let player_power = self.players[player].player_state.power as f64;

        let enemy_power = match target {
            FightTarget::Opponent => {
                let other_player = 1 - player;
                self.players[other_player].player_state.power
            }
            FightTarget::Enemy(enemy_idx) => self.enemies[enemy_idx].power,
        };

        let total_power = player_power + enemy_power as f64;
        let player_wins = rand::random::<f64>() < (player_power / total_power);

        if player_wins {
            match target {
                FightTarget::Opponent => {
                    let other_player = 1 - player;
                    self.players[other_player].player_state.health -= 1;
                    self.player_positions[other_player] = self.get_random_empty_node();
                }
                FightTarget::Enemy(enemy_idx) => {
                    let power_gain = (enemy_power as i32) / 2;
                    self.players[player].player_state.power += power_gain;
                    self.generate_enemy(enemy_idx);
                }
            }
        } else {
            // Player loses
            self.players[player].player_state.health -= 1;
            self.player_positions[player] = self.get_random_empty_node();
        }

        player_wins
    }

    /// Handles a gamble action for a player.
    /// Calls the player's `get_gamble_choice` function to get the player's choice.
    /// The player's power or health is then modified based on a random roll, or left unchanged if
    /// the player chooses to skip.
    fn handle_gamble(&mut self, player: usize) {
        let player_submission = &self.players[player];

        let response: GambleChoice = player_submission.call_function_0("get_gamble_choice");

        let roll = rand::random::<f64>();
        let player_state = &mut self.players[player].player_state;

        let value = match response {
            GambleChoice::Power => &mut player_state.power,
            GambleChoice::Health => &mut player_state.health,
            GambleChoice::Skip => return,
        };

        match roll {
            x if x < 0.1 => *value /= 2, // 10% chance to halve
            x if x < 0.2 => *value *= 2, // 10% chance to double
            x if x < 0.6 => *value += 1, // 40% chance to gain 1
            _ => *value -= 1,            // 40% chance to lose 1
        }
    }

    pub fn generate_game_map() -> (DiGraph<MapNode, i32>, Vec<NodeIndex>) {
        let mut map = DiGraph::new();
        let mut node_indices = Vec::new();

        let normal_node = map.add_node(MapNode::new(MapNodeType::Normal));
        let healing_node = map.add_node(MapNode::new(MapNodeType::Healing));
        let gamble_node = map.add_node(MapNode::new(MapNodeType::Gamble));
        let teleport_node = map.add_node(MapNode::new(MapNodeType::Teleport));

        map.add_edge(normal_node, healing_node, 0);
        map.add_edge(normal_node, gamble_node, 0);
        map.add_edge(normal_node, teleport_node, 0);

        node_indices.push(normal_node);
        node_indices.push(healing_node);
        node_indices.push(gamble_node);
        node_indices.push(teleport_node);

        (map, node_indices)
    }

    pub fn generate_enemies(&mut self) {
        self.generate_enemy(0);
        self.generate_enemy(1);
    }

    fn handle_enemy_turn(&mut self) {
        for i in 0..self.enemies.len() {
            let current_pos = self.enemy_positions[i];

            // Get all adjacent nodes that don't contain other enemies
            let available_moves: Vec<_> = self
                .map
                .edges_directed(current_pos, petgraph::Direction::Outgoing)
                .map(|edge| edge.target())
                .filter(|target| !self.enemy_positions.contains(target))
                .collect();

            if let Some(&new_pos) = available_moves.choose(&mut rand::rng()) {
                self.enemy_positions[i] = new_pos;

                let player_positions = self.player_positions;

                // Check if landed on player
                for (player_idx, &player_pos) in player_positions.iter().enumerate() {
                    if player_pos == new_pos {
                        self.handle_fight(player_idx, FightTarget::Enemy(i));
                    }
                }
            }
        }
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

    fn heal_player(&mut self, player: usize) {
        // heal players, but not above max health
        self.players[player].player_state.health = (self.players[player].player_state.health + 1)
            .min(self.players[player].player_state.max_health);
    }

    fn get_available_moves(&self, player: usize) -> WrappedChoices {
        let current_pos = self.player_positions[player];
        let mut node_types = Vec::new();
        let mut internal_choices = Vec::new();

        let mut edges: Vec<_> = self
            .map
            .edges_directed(current_pos, petgraph::Direction::Outgoing)
            .collect();

        edges.shuffle(&mut rand::rng());

        for (index, edge) in edges.iter().enumerate() {
            if let Some(node) = self.map.node_weight(edge.target()) {
                node_types.push(node.node_type);
                internal_choices[index] = edge.target();
            }
        }

        WrappedChoices {
            node_types,
            internal_choices,
        }
    }

    fn handle_player_movement(&mut self, player: usize, node_from: NodeIndex, node_to: NodeIndex) {
        println!(
            "Player {} moving from {:?} to {:?}",
            player, node_from, node_to
        );

        self.player_positions[player] = node_to;

        // Handle node effects first
        if let Some(node_data) = self.map.node_weight(node_to) {
            match node_data.node_type {
                MapNodeType::Healing => {
                    self.heal_player(player);
                }
                MapNodeType::Gamble => {
                    self.handle_gamble(player);
                }
                MapNodeType::Teleport => {
                    self.player_positions[player] = self.get_random_empty_node();
                }
                MapNodeType::Normal => {}
            }
        }

        // Then check for fights
        if let Some(fight_target) = self.check_for_fights(player) {
            return match fight_target {
                FightTarget::Opponent => {
                    self.handle_fight(player, fight_target);
                }
                FightTarget::Enemy(enemy) => {
                    let fight_info = FightInfo::Enemy(self.enemies[enemy]);
                    let response: PossiblePlayerFightChoices =
                        self.players[player].call_function("get_fight_choice", &fight_info);

                    match response {
                        PossiblePlayerFightChoices::Fight => {
                            self.handle_fight(player, fight_target);
                        }
                        PossiblePlayerFightChoices::Flee => {
                            let mut edges: Vec<_> = self
                                .map
                                .edges_directed(node_to, petgraph::Direction::Outgoing)
                                .collect();

                            edges.shuffle(&mut rand::rng());

                            let new_pos = edges
                                .iter()
                                .filter(|edge| {
                                    let node = edge.target();
                                    !self.player_positions.contains(&node)
                                        && !self.enemy_positions.contains(&node)
                                })
                                .map(|edge| edge.target())
                                .next()
                                .unwrap_or_else(|| self.get_random_empty_node());

                            // self.get_random_empty_node() always returns a
                            // node that is not occupied by an enemy or player,
                            // so the recursion will always terminate.
                            self.handle_player_movement(player, node_to, new_pos);
                        }
                    }
                }
            };
        }
    }
}
