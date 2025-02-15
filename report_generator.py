import sqlite3
import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns
from pathlib import Path
import os
import datetime

class TournamentReportGenerator:
    def __init__(self, db_path="results/results.sqlite"):
        self.db_path = db_path
        self.report_dir = Path("report")
        self.report_dir.mkdir(exist_ok=True)
        self.plots_dir = self.report_dir / "plots"
        self.plots_dir.mkdir(exist_ok=True)

    def _execute_query(self, query, conn):
        return pd.read_sql_query(query, conn)

    def generate_win_rates_chart(self, conn):
        query = """
        SELECT
            m.player_a,
            COUNT(CASE WHEN g.winner = 'player_a' THEN 1 END) as wins,
            COUNT(*) as total_games,
            ROUND(CAST(COUNT(CASE WHEN g.winner = 'player_a' THEN 1 END) AS FLOAT) / COUNT(*) * 100, 2) as win_percentage
        FROM matchups m
        JOIN games g ON m.id = g.matchup_id
        GROUP BY m.player_a
        ORDER BY win_percentage DESC
        """
        df = self._execute_query(query, conn)

        plt.figure(figsize=(12, 6))
        sns.barplot(data=df, x='player_a', y='win_percentage')
        plt.xticks(rotation=45)
        plt.title('Win Rates by Player')
        plt.tight_layout()
        plt.savefig(self.plots_dir / 'win_rates.png')
        plt.close()

        return df

    def generate_game_length_chart(self, conn):
        query = """
        SELECT
            m.player_a,
            m.player_b,
            ROUND(AVG(max_turn + 1), 2) as avg_game_length
        FROM matchups m
        JOIN games g ON m.id = g.matchup_id
        JOIN (
            SELECT game_id, MAX(turn_number) as max_turn
            FROM turns
            GROUP BY game_id
        ) t ON g.id = t.game_id
        GROUP BY m.player_a, m.player_b
        ORDER BY avg_game_length DESC
        """
        df = self._execute_query(query, conn)

        plt.figure(figsize=(12, 6))
        sns.barplot(data=df, x='avg_game_length', y=df.apply(lambda x: f"{x['player_a']} vs {x['player_b']}", axis=1))
        plt.title('Average Game Length by Matchup')
        plt.tight_layout()
        plt.savefig(self.plots_dir / 'game_lengths.png')
        plt.close()

        return df

    def generate_elo_ratings(self, conn):
        query = """
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
        ORDER BY final_elo DESC
        """
        df = self._execute_query(query, conn)

        plt.figure(figsize=(10, 6))
        sns.barplot(data=df, x='player_name', y='final_elo')
        plt.xticks(rotation=45)
        plt.title('Player ELO Ratings')
        plt.tight_layout()
        plt.savefig(self.plots_dir / 'elo_ratings.png')
        plt.close()

        return df

    def generate_head_to_head_stats(self, conn):
        query = """
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
        ORDER BY total_games DESC
        """
        return self._execute_query(query, conn)

    def generate_winning_combinations(self, conn):
        query = """
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
        ORDER BY wins DESC
        """
        df = self._execute_query(query, conn)

        plt.figure(figsize=(12, 6))
        sns.barplot(data=df.head(10), x='wins', y=df.apply(lambda x: f"{x['winner']} vs {x['loser']}", axis=1))
        plt.title('Most Common Winning Combinations')
        plt.tight_layout()
        plt.savefig(self.plots_dir / 'winning_combinations.png')
        plt.close()

        return df

    def generate_tie_statistics(self, conn):
        query = """
        SELECT
            m.player_a,
            m.player_b,
            COUNT(*) as tie_count,
            ROUND(CAST(COUNT(*) AS FLOAT) / COUNT(*) OVER (PARTITION BY m.id) * 100, 2) as tie_percentage
        FROM matchups m
        JOIN games g ON m.id = g.matchup_id
        WHERE g.winner = 'tie'
        GROUP BY m.player_a, m.player_b
        ORDER BY tie_count DESC
        """
        return self._execute_query(query, conn)

    def generate_game_length_extremes(self, conn):
        query = """
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
        LIMIT 10
        """
        return self._execute_query(query, conn)

    def generate_win_streaks(self, conn):
        query = """
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
        LIMIT 10
        """
        df = self._execute_query(query, conn)

        plt.figure(figsize=(10, 6))
        sns.barplot(data=df, x='player', y='streak_length')
        plt.xticks(rotation=45)
        plt.title('Longest Win Streaks by Player')
        plt.tight_layout()
        plt.savefig(self.plots_dir / 'win_streaks.png')
        plt.close()

        return df

    def generate_html_report(self):
        with sqlite3.connect(self.db_path) as conn:
            # Get all statistics
            win_rates_df = self.generate_win_rates_chart(conn)
            game_length_df = self.generate_game_length_chart(conn)
            elo_df = self.generate_elo_ratings(conn)
            h2h_df = self.generate_head_to_head_stats(conn)
            winning_combinations_df = self.generate_winning_combinations(conn)
            tie_stats_df = self.generate_tie_statistics(conn)
            game_extremes_df = self.generate_game_length_extremes(conn)
            win_streaks_df = self.generate_win_streaks(conn)

            html_content = f"""
            <html>
            <head>
                <title>Tournament Report - {datetime.datetime.now().strftime('%Y-%m-%d')}</title>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width,initial-scale=1.0" />
                <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/bulma@1.0.2/css/bulma.min.css">
            </head>
            <body>
                <h1>Tournament Report</h1>

                <div class="section">
                    <h2>Win Rates</h2>
                    <img src="plots/win_rates.png">
                    {win_rates_df.to_html()}
                </div>

                <div class="section">
                    <h2>Head-to-Head Statistics</h2>
                    {h2h_df.to_html()}
                </div>

                <div class="section">
                    <h2>Most Common Winning Combinations</h2>
                    <img src="plots/winning_combinations.png">
                    {winning_combinations_df.to_html()}
                </div>

                <div class="section">
                    <h2>Game Lengths</h2>
                    <img src="plots/game_lengths.png">
                    {game_length_df.to_html()}
                </div>

                <div class="section">
                    <h2>Longest Games</h2>
                    {game_extremes_df.to_html()}
                </div>

                <div class="section">
                    <h2>Tie Statistics</h2>
                    {tie_stats_df.to_html()}
                </div>

                <div class="section">
                    <h2>Win Streaks</h2>
                    <img src="plots/win_streaks.png">
                    {win_streaks_df.to_html()}
                </div>

                <div class="section">
                    <h2>ELO Ratings</h2>
                    <img src="plots/elo_ratings.png">
                    {elo_df.to_html()}
                </div>
            </body>
            </html>
            """

            with open(self.report_dir / "report.html", "w") as f:
                f.write(html_content)

def main():
    generator = TournamentReportGenerator()
    generator.generate_html_report()

if __name__ == "__main__":
    main()
