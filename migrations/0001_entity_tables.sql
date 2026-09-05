-- Entity tables: teams, players, seasons, games
-- Order: teams → seasons → players → games (games references teams via FK)

CREATE TABLE teams (
    team_id     BIGINT PRIMARY KEY,
    full_name   TEXT NOT NULL,
    common_name TEXT NOT NULL,
    place_name  TEXT NOT NULL,
    abbrev      TEXT NOT NULL UNIQUE
);

CREATE TABLE seasons (
    season_id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    season_year             INTEGER NOT NULL UNIQUE,
    start_date              DATE,
    end_date                DATE,
    regular_season_end_date DATE
);

CREATE TABLE players (
    player_id           BIGINT PRIMARY KEY,
    first_name          TEXT NOT NULL,
    last_name           TEXT NOT NULL,
    position            TEXT,
    shoots_catches      TEXT,
    current_team_abbrev TEXT,
    birth_date          DATE,
    height_cm           SMALLINT,
    weight_kg           SMALLINT,
    draft_year          SMALLINT,
    draft_round         SMALLINT,
    draft_pick          SMALLINT,
    draft_team_abbrev   TEXT,
    draft_overall_pick  SMALLINT
);

CREATE TABLE games (
    game_id         BIGINT PRIMARY KEY,
    season          INTEGER NOT NULL,
    game_date       DATE NOT NULL,
    start_time_utc  TIMESTAMPTZ,
    home_team_id    BIGINT NOT NULL REFERENCES teams(team_id),
    away_team_id    BIGINT NOT NULL REFERENCES teams(team_id),
    game_type       SMALLINT NOT NULL,
    venue           TEXT,
    venue_location  TEXT,
    game_state      TEXT,
    home_score      SMALLINT,
    away_score      SMALLINT
);

CREATE INDEX idx_games_season    ON games(season);
CREATE INDEX idx_games_game_date ON games(game_date);
CREATE INDEX idx_games_home_team ON games(home_team_id);
CREATE INDEX idx_games_away_team ON games(away_team_id);
CREATE INDEX idx_players_current_team ON players(current_team_abbrev);
