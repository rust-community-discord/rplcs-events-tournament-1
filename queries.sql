-- Overall win rates per player
SELECT
    m.player_a,
    COUNT(CASE WHEN g.winner = 'player_a' THEN 1 END) as wins,
    COUNT(*) as total_games,
    ROUND(CAST(COUNT(CASE WHEN g.winner = 'player_a' THEN 1 END) AS FLOAT) / COUNT(*) * 100, 2) as win_percentage
FROM matchups m
JOIN games g ON m.id = g.matchup_id
GROUP BY m.player_a
ORDER BY win_percentage DESC;

-- Head-to-head statistics
SELECT
    m.player_a,
    m.player_b,
    COUNT(CASE WHEN g.winner = 'player_a' THEN 1 END) as player_a_wins,
    COUNT(CASE WHEN g.winner = 'player_b' THEN 1 END) as player_b_wins,
    COUNT(CASE WHEN g.winner = 'tie' THEN 1 END) as ties,
    COUNT(*) as total_games
FROM matchups m
JOIN games g ON m.id = g.matchup_id
GROUP BY m.player_a, m.player_b
ORDER BY total_games DESC;

-- Average game length (turns) per matchup
SELECT
    m.player_a,
    m.player_b,
    COUNT(DISTINCT g.id) as games_played,
    ROUND(AVG(max_turn + 1), 2) as avg_game_length
FROM matchups m
JOIN games g ON m.id = g.matchup_id
JOIN (
    SELECT game_id, MAX(turn_number) as max_turn
    FROM turns
    GROUP BY game_id
) t ON g.id = t.game_id
GROUP BY m.player_a, m.player_b
ORDER BY avg_game_length DESC;

-- Most common winning player combinations
SELECT
    CASE
        WHEN g.winner = 'player_a' THEN m.player_a
        WHEN g.winner = 'player_b' THEN m.player_b
    END as winner,
    CASE
        WHEN g.winner = 'player_a' THEN m.player_b
        WHEN g.winner = 'player_b' THEN m.player_a
    END as loser,
    COUNT(*) as wins
FROM matchups m
JOIN games g ON m.id = g.matchup_id
WHERE g.winner != 'tie'
GROUP BY winner, loser
ORDER BY wins DESC;

-- Matchups with most ties
SELECT
    m.player_a,
    m.player_b,
    COUNT(*) as tie_count,
    ROUND(CAST(COUNT(*) AS FLOAT) / COUNT(*) OVER (PARTITION BY m.id) * 100, 2) as tie_percentage
FROM matchups m
JOIN games g ON m.id = g.matchup_id
WHERE g.winner = 'tie'
GROUP BY m.player_a, m.player_b
ORDER BY tie_count DESC;

-- Longest and shortest games
SELECT
    m.player_a,
    m.player_b,
    g.id as game_id,
    MAX(t.turn_number) + 1 as game_length,
    g.winner
FROM matchups m
JOIN games g ON m.id = g.matchup_id
JOIN turns t ON g.id = t.game_id
GROUP BY g.id, m.player_a, m.player_b, g.winner
ORDER BY game_length DESC
LIMIT 10;

-- Calculate ELO ratings (simplified, non-recursive version)
WITH all_players AS (
    SELECT player_name, 1500 as base_elo
    FROM (
        SELECT player_a as player_name FROM matchups
        UNION
        SELECT player_b FROM matchups
    )
)
SELECT
    p.player_name,
    p.base_elo + (
        COALESCE(SUM(
            CASE
                WHEN g.winner = 'player_a' AND m.player_a = p.player_name THEN 32
                WHEN g.winner = 'player_b' AND m.player_b = p.player_name THEN 32
                WHEN g.winner != 'tie' AND
                     (m.player_a = p.player_name OR m.player_b = p.player_name) THEN -32
                ELSE 0
            END
        ), 0)
    ) as final_elo
FROM all_players p
LEFT JOIN matchups m ON p.player_name IN (m.player_a, m.player_b)
LEFT JOIN games g ON m.id = g.matchup_id
GROUP BY p.player_name
ORDER BY final_elo DESC;

-- Win streaks
WITH game_results AS (
    SELECT
        m.player_a,
        m.player_b,
        g.id,
        g.winner,
        ROW_NUMBER() OVER (ORDER BY g.id) as game_number
    FROM matchups m
    JOIN games g ON m.id = g.matchup_id
),
win_streaks AS (
    SELECT
        CASE
            WHEN winner = 'player_a' THEN player_a
            WHEN winner = 'player_b' THEN player_b
        END as player,
        game_number,
        game_number - ROW_NUMBER() OVER (
            PARTITION BY
                CASE
                    WHEN winner = 'player_a' THEN player_a
                    WHEN winner = 'player_b' THEN player_b
                END
            ORDER BY game_number
        ) as streak_group
    FROM game_results
    WHERE winner != 'tie'
)
SELECT
    player,
    COUNT(*) as streak_length
FROM win_streaks
GROUP BY player, streak_group
HAVING COUNT(*) > 1
ORDER BY streak_length DESC
LIMIT 10;
