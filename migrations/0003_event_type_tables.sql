-- Event-type child tables: goals, shots, hits, blocks, penalties, faceoffs
-- Each table has a 1:1 FK to events(id) via event_id PRIMARY KEY
-- All player ID columns are nullable (EN goals, unassisted goals, offsetting penalties, etc.)

CREATE TABLE goals (
    event_id            BIGINT PRIMARY KEY REFERENCES events(id),
    scorer_player_id    BIGINT,
    assist1_player_id   BIGINT,
    assist2_player_id   BIGINT,
    goalie_id           BIGINT,
    shot_type           TEXT
);

CREATE TABLE shots (
    event_id                BIGINT PRIMARY KEY REFERENCES events(id),
    shooting_player_id      BIGINT,
    goalie_in_net_id        BIGINT,
    shot_type               TEXT
);

CREATE TABLE hits (
    event_id            BIGINT PRIMARY KEY REFERENCES events(id),
    hitting_player_id   BIGINT,
    hittee_player_id    BIGINT
);

CREATE TABLE blocks (
    event_id                BIGINT PRIMARY KEY REFERENCES events(id),
    blocking_player_id      BIGINT,
    shooting_player_id      BIGINT
);

CREATE TABLE penalties (
    event_id                BIGINT PRIMARY KEY REFERENCES events(id),
    committed_by_player_id  BIGINT,
    drawn_by_player_id      BIGINT,
    infraction_type         TEXT,
    duration_minutes        SMALLINT
);

CREATE TABLE faceoffs (
    event_id            BIGINT PRIMARY KEY REFERENCES events(id),
    winning_player_id   BIGINT,
    losing_player_id    BIGINT
);

CREATE INDEX idx_goals_scorer   ON goals(scorer_player_id);
CREATE INDEX idx_shots_shooter  ON shots(shooting_player_id);
