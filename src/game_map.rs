use anyhow::{Context, Result, anyhow, bail};
use layout::{
    backends::svg::SVGWriter,
    core::{
        base::Orientation, color::Color, geometry::Point, style::StyleAttr, utils::save_to_file,
    },
    std_shapes::shapes::{Arrow, Element, LineEndKind, ShapeKind},
    topo::layout::VisualGraph,
};
use petgraph::{
    graph::{DiGraph, EdgeReference, NodeIndex},
    visit::EdgeRef,
};
use rand::{prelude::*, rngs::StdRng};
use rplcs_events::tournament_1::{MapNodeType, PlayerState};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

mod tests;

pub const MAX_DEGREE: usize = 4;
pub const MIN_DEGREE: usize = 3;

pub struct GameMap {
    graph: DiGraph<MapNodeType, ()>,
}

impl GameMap {
    pub fn new(rng: &mut StdRng) -> Result<Self> {
        let mut map = Self {
            graph: DiGraph::new(),
        };

        // Create nodes
        let num_nodes = rng.random_range(12..=16);
        let num_teleport_nodes = 1;
        let num_healing_nodes = rng.random_range(1..=2);
        let num_gamble_nodes = rng.random_range(1..=2);
        let num_normal_nodes =
            num_nodes - num_teleport_nodes - num_healing_nodes - num_gamble_nodes;

        // Add all nodes first
        for _ in 0..num_teleport_nodes {
            map.graph.add_node(MapNodeType::Teleport);
        }
        for _ in 0..num_healing_nodes {
            map.graph.add_node(MapNodeType::Healing);
        }
        for _ in 0..num_gamble_nodes {
            map.graph.add_node(MapNodeType::Gamble);
        }
        for _ in 0..num_normal_nodes {
            map.graph.add_node(MapNodeType::Normal);
        }

        // Keep adding edges until all nodes have minimum degree
        while let Some(node) = map
            .node_indices()
            .into_iter()
            .find(|&node| map.get_node_degree(node) < MIN_DEGREE)
        {
            let target = {
                let available_targets: Vec<_> = map
                    .node_indices()
                    .into_iter()
                    .filter(|&target| {
                        // Skip if already connected
                        let already_connected = map.graph.find_edge(node, target).is_some()
                            || map.graph.find_edge(target, node).is_some();
                        if already_connected {
                            return false;
                        }

                        // Skip self-target if already has a loop
                        if node == target && !map.get_loops(target).is_empty() {
                            return false;
                        }

                        true
                    })
                    .collect();

                if available_targets.is_empty() {
                    bail!("Failed to find valid targets for node {:?}", node);
                }

                available_targets
                    .choose(rng)
                    .ok_or(anyhow!("Failed to choose target for node {:?}", node))?
                    .clone()
            };

            // Try to make room if target is at max degree
            if map.get_node_degree(target) >= MAX_DEGREE {
                // Remove loops or edges
                let target_loops = map.get_loops(target);

                if !target_loops.is_empty() {
                    for loop_edge in target_loops {
                        map.graph.remove_edge(loop_edge);
                    }
                } else {
                    let (edge_id, other) = map
                        .get_outgoing_edges(target)
                        .into_iter()
                        .map(|e| (e.id(), e.target()))
                        .next()
                        .ok_or(anyhow!(
                            "Failed to find outgoing edges for node {:?}",
                            target
                        ))?;

                    map.graph.remove_edge(edge_id);
                    if let Some(back_edge) = map
                        .get_outgoing_edges(other)
                        .into_iter()
                        .find(|e| e.target() == target)
                    {
                        map.graph.remove_edge(back_edge.id());
                    }
                }
            }

            map.graph.add_edge(node, target, ());

            // Add bidirectional edge if node or target is unbalanced or if random roll succeeds
            if !map.is_node_balanced(target) || !map.is_node_balanced(node) || rng.random_bool(0.85)
            {
                map.graph.add_edge(target, node, ());
            }
        }

        Ok(map)
    }

    #[cfg(test)]
    pub fn from_graph(graph: DiGraph<MapNodeType, ()>) -> Self {
        Self { graph }
    }

    // Replace graph() with specific utility methods
    pub fn get_node_type(&self, node: NodeIndex) -> Option<MapNodeType> {
        self.graph.node_weight(node).copied()
    }

    #[cfg(test)]
    pub fn node_weights(&self) -> Vec<MapNodeType> {
        self.graph.node_weights().copied().collect()
    }

    pub fn get_outgoing_edges(&self, node: NodeIndex) -> Vec<EdgeReference<'_, ()>> {
        self.graph
            .edges_directed(node, petgraph::Direction::Outgoing)
            .collect()
    }

    #[cfg(test)]
    pub fn get_incoming_edges(&self, node: NodeIndex) -> Vec<EdgeReference<'_, ()>> {
        self.graph
            .edges_directed(node, petgraph::Direction::Incoming)
            .collect()
    }

    pub fn get_available_moves(
        &self,
        from: NodeIndex,
        blocked_positions: &[NodeIndex],
    ) -> Vec<NodeIndex> {
        self.get_outgoing_nodes(from)
            .into_iter()
            .filter(|target| !blocked_positions.contains(target))
            .collect()
    }

    pub fn shuffle_available_moves(
        &self,
        from: NodeIndex,
        blocked_positions: &[NodeIndex],
        rng: &mut StdRng,
    ) -> Vec<NodeIndex> {
        let mut moves = self.get_available_moves(from, blocked_positions);
        moves.shuffle(rng);
        moves
    }

    pub fn get_random_empty_node(
        &self,
        blocked_positions: &[NodeIndex],
        rng: &mut StdRng,
    ) -> Option<NodeIndex> {
        let mut available: Vec<_> = self
            .node_indices()
            .into_iter()
            .filter(|&node| {
                let is_teleport = matches!(self.get_node_type(node), Some(MapNodeType::Teleport));
                !blocked_positions.contains(&node) && !is_teleport
            })
            .collect();

        if available.is_empty() {
            None
        } else {
            available.shuffle(rng);
            Some(available[0])
        }
    }

    pub fn node_indices(&self) -> Vec<NodeIndex> {
        self.graph.node_indices().collect()
    }

    pub fn get_node_degree(&self, node: NodeIndex) -> usize {
        let outgoing = self.graph.edges_directed(node, petgraph::Outgoing).count();
        let incoming = self.graph.edges_directed(node, petgraph::Incoming).count();
        let loops = self.get_loops(node).len();

        let degree = outgoing.max(incoming);
        degree - (loops / 2)
    }

    pub fn get_outgoing_nodes(&self, node: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .edges_directed(node, petgraph::Direction::Outgoing)
            .map(|e| e.target())
            .collect()
    }

    pub fn get_loops(&self, node: NodeIndex) -> Vec<petgraph::graph::EdgeIndex> {
        self.graph
            .edges_directed(node, petgraph::Direction::Outgoing)
            .filter(|e| e.target() == node)
            .map(|e| e.id())
            .collect()
    }

    pub fn is_node_balanced(&self, node: NodeIndex) -> bool {
        let outgoing = self.graph.edges_directed(node, petgraph::Outgoing).count();
        let incoming = self.graph.edges_directed(node, petgraph::Incoming).count();

        outgoing == incoming
    }

    pub fn render_to_file(
        &self,
        player_positions: [NodeIndex; 2],
        enemy_positions: [NodeIndex; 2],
        players: &[PlayerState; 2],
        enemies: &[PlayerState; 2],
        path: &Path,
    ) -> Result<()> {
        let mut visual = VisualGraph::new(Orientation::TopToBottom);

        // Create nodes with custom styles
        let mut node_map = HashMap::new();
        for node_idx in self.graph.node_indices() {
            let node_type = self.graph.node_weight(node_idx).unwrap();
            let fill_color = match node_type {
                MapNodeType::Teleport => 0xb3dbbaff,
                MapNodeType::Healing => 0x4cc037ff,
                MapNodeType::Normal => 0xcfcfcfff,
                MapNodeType::Gamble => 0xf1c232ff,
            };

            let label = {
                let mut label = node_idx.index().to_string();

                if player_positions[0] == node_idx {
                    let player = &players[0];
                    label.push_str(&format!(
                        "\nA {}/{} {}",
                        player.health, player.max_health, player.power
                    ));
                }
                if player_positions[1] == node_idx {
                    let player = &players[1];
                    label.push_str(&format!(
                        "\nB {}/{} {}",
                        player.health, player.max_health, player.power
                    ));
                }
                if enemy_positions[0] == node_idx {
                    let enemy = &enemies[0];
                    label.push_str(&format!(
                        "\nE {}/{} {}",
                        enemy.health, enemy.max_health, enemy.power
                    ));
                }
                if enemy_positions[1] == node_idx {
                    let enemy = &enemies[1];
                    label.push_str(&format!(
                        "\nE {}/{} {}",
                        enemy.health, enemy.max_health, enemy.power
                    ));
                }

                label
            };

            let element = Element::create(
                ShapeKind::Box(label),
                StyleAttr::new(
                    Color::new(0x000000ff),
                    1,
                    Some(Color::new(fill_color)),
                    3,
                    32,
                ),
                Orientation::TopToBottom,
                Point::new(100.0, 100.0),
            );

            let node = visual.add_node(element);
            node_map.insert(node_idx, node);
        }

        let mut drawn_edges = HashSet::new();

        // Create edges with arrows
        for edge in self.graph.edge_indices() {
            let (source, target) = self.graph.edge_endpoints(edge).unwrap();

            if drawn_edges.contains(&(source, target)) {
                continue;
            }

            let visual_source = node_map[&source];
            let visual_target = node_map[&target];

            let has_bidirectional_loop = self
                .graph
                .edges_directed(source, petgraph::Direction::Outgoing)
                .filter(|e| e.target() == target)
                .count()
                == 2;

            let is_there_bidirectional_edge = {
                let source_edges = self
                    .graph
                    .edges_directed(source, petgraph::Direction::Outgoing)
                    .find(|e| e.target() == target);
                let target_edges = self
                    .graph
                    .edges_directed(target, petgraph::Direction::Outgoing)
                    .find(|e| e.target() == source);
                source_edges.is_some() && target_edges.is_some()
            };

            let arrow = if is_there_bidirectional_edge || has_bidirectional_loop {
                let mut arrow = Arrow::default();
                arrow.end = LineEndKind::None;
                arrow
            } else {
                Arrow::default()
            };

            visual.add_edge(arrow, visual_source, visual_target);
            drawn_edges.insert((source, target));
            drawn_edges.insert((target, source));
        }

        let mut writer = SVGWriter::new();
        visual.do_it(false, false, false, &mut writer);
        std::fs::create_dir_all(path.parent().context("Failed to get parent directory")?)?;
        save_to_file(path.to_str().context("Invalid path")?, &writer.finalize())?;
        Ok(())
    }
}
