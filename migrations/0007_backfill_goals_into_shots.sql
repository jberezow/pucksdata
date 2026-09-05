-- Backfill: insert a shots row for every goal event that has no shots row.
--
-- Goals are shots on net — the transform layer now double-inserts into both
-- goals and shots for new data. This migration patches historical data.
--
-- Column mapping:
--   goals.event_id          → shots.event_id       (same surrogate FK)
--   goals.scorer_player_id  → shots.shooting_player_id
--   goals.goalie_id         → shots.goalie_in_net_id  (different column names, same role)
--   goals.shot_type         → shots.shot_type
--
-- Idempotent: ON CONFLICT (event_id) DO NOTHING ensures re-runs are safe.
INSERT INTO shots (event_id, shooting_player_id, goalie_in_net_id, shot_type)
SELECT g.event_id, g.scorer_player_id, g.goalie_id, g.shot_type
FROM goals g
WHERE NOT EXISTS (
    SELECT 1 FROM shots s WHERE s.event_id = g.event_id
)
ON CONFLICT (event_id) DO NOTHING;
