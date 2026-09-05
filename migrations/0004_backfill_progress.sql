-- Checkpoint table for resumable backfill orchestration.
-- Tracks per-game completion status so interrupted backfills resume cleanly.
-- Status values: 'pending', 'done', 'failed' (no 'in_progress' — killed runs leave 'pending').
CREATE TABLE backfill_progress (
    game_id     BIGINT PRIMARY KEY,
    season      INTEGER NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_backfill_progress_status ON backfill_progress(status);
CREATE INDEX idx_backfill_progress_season ON backfill_progress(season);
