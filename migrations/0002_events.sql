-- Events base table: parent table for all event-type child tables
-- situationCode is decoded during ingestion into five columns stored here.

CREATE TABLE events (
    id                    BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id               BIGINT NOT NULL REFERENCES games(game_id),
    event_id_in_game      INTEGER NOT NULL,
    period                SMALLINT NOT NULL,
    period_type           TEXT NOT NULL,
    time_in_period        TEXT NOT NULL,
    event_type            TEXT NOT NULL,
    x_coord               SMALLINT,
    y_coord               SMALLINT,
    zone_code             TEXT,
    event_owner_team_id   BIGINT REFERENCES teams(team_id),
    away_goalie_present   BOOLEAN NOT NULL DEFAULT TRUE,
    away_skater_count     SMALLINT NOT NULL DEFAULT 5,
    home_skater_count     SMALLINT NOT NULL DEFAULT 5,
    home_goalie_present   BOOLEAN NOT NULL DEFAULT TRUE,
    strength              TEXT,
    UNIQUE (game_id, event_id_in_game)
);

CREATE INDEX idx_events_game_id          ON events(game_id);
CREATE INDEX idx_events_event_type       ON events(event_type);
CREATE INDEX idx_events_event_owner_team ON events(event_owner_team_id);
