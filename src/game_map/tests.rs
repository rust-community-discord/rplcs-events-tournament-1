#![cfg(test)]

use std::path::Path;

use petgraph::graph::DiGraph;
use petgraph::{graph::NodeIndex, visit::EdgeRef};
use quickcheck::{quickcheck, TestResult};
use rand::{rngs::StdRng, SeedableRng};
use rplcs_events::tournament_1::{MapNodeType, PlayerState};

use crate::game_map::GameMap;

use super::{MAX_DEGREE, MIN_DEGREE};

fn validate_map(map: &GameMap, seed: i64) -> TestResult {
    // Check node counts
    let node_weights: Vec<_> = map.node_weights();
    let node_count = node_weights.len();

    if !(12..=16).contains(&node_count) {
        return TestResult::error(format!(
            "Map should have between 12 and 16 nodes (has {}) [seed: {}]",
            node_count, seed
        ));
    }

    let teleport_count = node_weights
        .iter()
        .filter(|&&n| matches!(n, MapNodeType::Teleport))
        .count();
    if teleport_count != 1 {
        return TestResult::error(format!(
            "Map should have exactly 1 teleport node (has {}) [seed: {}]",
            teleport_count, seed
        ));
    }

    let healing_count = node_weights
        .iter()
        .filter(|&&n| matches!(n, MapNodeType::Healing))
        .count();
    if healing_count < 1 {
        return TestResult::error(format!(
            "Map should have at least 1 healing node (has {}) [seed: {}]",
            healing_count, seed
        ));
    }

    let gamble_count = node_weights
        .iter()
        .filter(|&&n| matches!(n, MapNodeType::Gamble))
        .count();
    if gamble_count < 1 {
        return TestResult::error(format!(
            "Map should have at least 1 gamble node (has {}) [seed: {}]",
            gamble_count, seed
        ));
    }

    // Check edge constraints
    for node in map.node_indices() {
        let loops = map.get_loops(node).len();
        if loops >= 3 {
            return TestResult::error(format!(
                "Node {:?} has invalid number of loops: {} [seed: {}]",
                node, loops, seed
            ));
        }

        let node_degree = map.get_node_degree(node);
        if node_degree < MIN_DEGREE || node_degree > MAX_DEGREE {
            return TestResult::error(format!(
                "Node {:?} has invalid degree: {} [seed: {}]",
                node, node_degree, seed
            ));
        }
    }

    TestResult::passed()
}

quickcheck! {
    fn test_map_generation(seed: i64) -> TestResult {
        let mut rng = StdRng::seed_from_u64(seed as u64);
        let map = GameMap::new(&mut rng).expect("Failed to generate map");
        validate_map(&map, seed)
    }
}

quickcheck! {
    fn test_node_degree(edges: Vec<(u8, u8)>) -> TestResult {
        // Create a small test graph with at most 5 nodes to keep tests manageable
        let node_count = 5;

        // Skip empty edge lists
        if edges.is_empty() {
            return TestResult::discard();
        }

        let mut graph = DiGraph::new();

        // Add nodes
        for _ in 0..node_count {
            graph.add_node(MapNodeType::Normal);
        }

        // Add edges, mapping the u8 values to valid node indices
        for (from, to) in edges {
            let from = NodeIndex::new((from as usize) % node_count);
            let to = NodeIndex::new((to as usize) % node_count);
            graph.add_edge(from, to, ());
        }

        let map = GameMap::from_graph(graph);

        // Verify degree calculation for each node
        for node in map.node_indices() {
            let outgoing = map.get_outgoing_edges(node).len();
            let incoming = map.get_incoming_edges(node).len();
            let loops = map.get_outgoing_edges(node)
                .into_iter()
                .filter(|e| e.target() == node)
                .count();

            let expected = outgoing.max(incoming) - (loops / 2);
            let actual = map.get_node_degree(node);

            if expected != actual {
                return TestResult::error(format!(
                    "Node {:?} degree mismatch: expected {}, got {} (out: {}, in: {}, loops: {})",
                    node, expected, actual, outgoing, incoming, loops
                ));
            }
        }

        TestResult::passed()
    }
}

#[test]
#[ignore]
fn test_specific_seeds() {
    // Test some edge cases and known problematic seeds
    let problem_seeds = [-478597674355546704i64];

    for &seed in &problem_seeds {
        let mut rng = StdRng::seed_from_u64(seed as u64);
        let map = GameMap::new(&mut rng).expect("Failed to generate map");
        map.render_to_file(
            [NodeIndex::new(0), NodeIndex::new(1)],
            [NodeIndex::new(2), NodeIndex::new(3)],
            &[PlayerState::default(), PlayerState::default()],
            &[PlayerState::default(), PlayerState::default()],
            Path::new("problem_map.svg"),
        )
        .expect("Failed to render map");

        let result = validate_map(&map, seed);
        if result.is_error() {
            panic!("Seed {} failed: {:?}", seed, result);
        }
    }
}
