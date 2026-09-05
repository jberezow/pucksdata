mod common;

#[test]
fn test_is_game_completed_unit() {
    assert!(
        pucksdata::process::sync::is_game_completed("OFF"),
        "OFF should be completed"
    );
    assert!(
        pucksdata::process::sync::is_game_completed("OVER"),
        "OVER should be completed"
    );
    assert!(
        pucksdata::process::sync::is_game_completed("FINAL"),
        "FINAL should be completed"
    );
    assert!(
        !pucksdata::process::sync::is_game_completed("LIVE"),
        "LIVE must not be completed"
    );
    assert!(
        !pucksdata::process::sync::is_game_completed("PPD"),
        "PPD must not be completed"
    );
    assert!(
        !pucksdata::process::sync::is_game_completed("FUT"),
        "FUT must not be completed"
    );
    assert!(
        !pucksdata::process::sync::is_game_completed(""),
        "empty string must not be completed"
    );
}

#[tokio::test]
async fn test_query_sync_candidates_detects_gap() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99911, 'Sync Home', 'SyncH', 'Testville', 'SNH'),
                (99912, 'Sync Away', 'SyncA', 'Testville', 'SNA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9991000001, 99991, '2020-01-01', 99911, 99912, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    let candidates = pucksdata::process::sync::query_sync_candidates(pool, None)
        .await
        .unwrap();
    let ids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
    assert!(
        ids.contains(&9991000001),
        "game with no events must appear in gap detection"
    );

    sqlx::query!(
        "INSERT INTO events (game_id, event_id_in_game, period, period_type, time_in_period, event_type)
         VALUES (9991000001, 1, 1, 'REG', '00:00', 'goal')
         ON CONFLICT (game_id, event_id_in_game) DO NOTHING"
    ).execute(pool).await.unwrap();

    let candidates2 = pucksdata::process::sync::query_sync_candidates(pool, None)
        .await
        .unwrap();
    let ids2: Vec<i64> = candidates2.iter().map(|(id, _)| *id).collect();
    assert!(
        !ids2.contains(&9991000001),
        "game with events must not appear in gap detection (idempotent)"
    );

    sqlx::query!("DELETE FROM events WHERE game_id = 9991000001")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id = 9991000001")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99911, 99912)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_query_sync_candidates_respects_acknowledged_gaps() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99917, 'Sync Home 4', 'SyncH4', 'Testville', 'SH4'),
                (99918, 'Sync Away 4', 'SyncA4', 'Testville', 'SA4')
         ON CONFLICT (team_id) DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9991000006, 99995, '2020-01-01', 99917, 99918, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO backfill_progress (game_id, season, status)
         VALUES (9991000006, 99995, 'done')
         ON CONFLICT (game_id) DO UPDATE SET status = EXCLUDED.status",
    )
    .execute(pool)
    .await
    .unwrap();

    let candidates = pucksdata::process::sync::query_sync_candidates(pool, None)
        .await
        .unwrap();
    assert!(!candidates.iter().any(|(id, _)| *id == 9991000006));

    sqlx::query("DELETE FROM backfill_progress WHERE game_id = 9991000006")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM games WHERE game_id = 9991000006")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM teams WHERE team_id IN (99917, 99918)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_query_sync_candidates_from_date_filter() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99913, 'Sync Home 2', 'SyncH2', 'Testville', 'SH2'),
                (99914, 'Sync Away 2', 'SyncA2', 'Testville', 'SA2')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9991000002, 99992, '2020-01-01', 99913, 99914, 2, 'OFF'),
                (9991000003, 99993, '2022-06-01', 99913, 99914, 2, 'FINAL')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    let cutoff = time::Date::from_calendar_date(2022, time::Month::January, 1).unwrap();
    let candidates = pucksdata::process::sync::query_sync_candidates(pool, Some(cutoff))
        .await
        .unwrap();
    let ids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();

    assert!(
        !ids.contains(&9991000002),
        "game before cutoff must be excluded by from_date filter"
    );
    assert!(
        ids.contains(&9991000003),
        "game after cutoff must be included by from_date filter"
    );

    sqlx::query!("DELETE FROM games WHERE game_id IN (9991000002, 9991000003)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99913, 99914)")
        .execute(pool)
        .await
        .unwrap();
}

// Candidate discovery leaves game-state handling to the sync orchestrator.
#[tokio::test]
async fn test_query_sync_candidates_includes_null_state() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99915, 'Sync Home 3', 'SyncH3', 'Testville', 'SH3'),
                (99916, 'Sync Away 3', 'SyncA3', 'Testville', 'SA3')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type)
         VALUES (9991000004, 99994, '2020-01-01', 99915, 99916, 2)
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    let candidates = pucksdata::process::sync::query_sync_candidates(pool, None)
        .await
        .unwrap();
    let matching: Vec<_> = candidates
        .iter()
        .filter(|(id, _)| *id == 9991000004)
        .collect();

    assert!(
        !matching.is_empty(),
        "game with NULL state must appear in candidates (Rust filters it)"
    );
    assert!(
        matching[0].1.is_none(),
        "game_state should be None for NULL state"
    );

    sqlx::query!("DELETE FROM games WHERE game_id = 9991000004")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99915, 99916)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_sync_state_upsert() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!("DELETE FROM sync_state WHERE key = 'singleton'")
        .execute(pool)
        .await
        .unwrap();

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99921, 'State Home', 'StateH', 'Testville', 'STH'),
                (99922, 'State Away', 'StateA', 'Testville', 'STA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    // Exercise the metadata upsert directly to avoid live API calls in run_sync.
    let now = time::OffsetDateTime::now_utc();
    let processed_count: i32 = 3;
    sqlx::query!(
        r#"INSERT INTO sync_state (key, last_sync_at, last_sync_games, updated_at)
       VALUES ('singleton', $1, $2, $1)
       ON CONFLICT (key) DO UPDATE
         SET last_sync_at    = EXCLUDED.last_sync_at,
             last_sync_games = EXCLUDED.last_sync_games,
             updated_at      = EXCLUDED.updated_at"#,
        now,
        processed_count
    )
    .execute(pool)
    .await
    .unwrap();

    let row = sqlx::query!(
        "SELECT key, last_sync_games, last_sync_at FROM sync_state WHERE key = 'singleton'"
    )
    .fetch_one(pool)
    .await
    .unwrap();

    assert_eq!(row.key, "singleton", "sync_state key must be 'singleton'");
    assert_eq!(
        row.last_sync_games,
        Some(3),
        "last_sync_games must equal processed count"
    );
    assert!(
        row.last_sync_at.is_some(),
        "last_sync_at must be set after upsert"
    );

    let now2 = time::OffsetDateTime::now_utc();
    let processed_count2: i32 = 7;
    sqlx::query!(
        r#"INSERT INTO sync_state (key, last_sync_at, last_sync_games, updated_at)
       VALUES ('singleton', $1, $2, $1)
       ON CONFLICT (key) DO UPDATE
         SET last_sync_at    = EXCLUDED.last_sync_at,
             last_sync_games = EXCLUDED.last_sync_games,
             updated_at      = EXCLUDED.updated_at"#,
        now2,
        processed_count2
    )
    .execute(pool)
    .await
    .unwrap();

    let row2 = sqlx::query!("SELECT last_sync_games FROM sync_state WHERE key = 'singleton'")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        row2.last_sync_games,
        Some(7),
        "second upsert must update last_sync_games to 7"
    );

    sqlx::query!("DELETE FROM sync_state WHERE key = 'singleton'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99921, 99922)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_advisory_lock_single_instance() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    let guard = pucksdata::process::sync::acquire_daemon_lock(pool).await;
    assert!(guard.is_ok(), "first acquire_daemon_lock() must succeed");
    let _guard = guard.unwrap();

    let guard2 = pucksdata::process::sync::acquire_daemon_lock(pool).await;
    assert!(
        guard2.is_err(),
        "second acquire_daemon_lock() on same pool must return Err (lock already held)"
    );
}
