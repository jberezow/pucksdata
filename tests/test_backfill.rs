mod common;

#[tokio::test]
async fn test_backfill_progress_seed_idempotent() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99901, 'Backfill Home', 'Home', 'Testville', 'BFH'),
                (99902, 'Backfill Away', 'Away', 'Testville', 'BFA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000001, 99991, '2099-01-01', 99901, 99902, 2, 'OFF'),
                (9990000002, 99991, '2099-01-02', 99901, 99902, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99991))
        .await
        .unwrap();
    let count1: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM backfill_progress WHERE season = 99991")
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);
    assert_eq!(count1, 2, "first seed should insert 2 rows");

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99991))
        .await
        .unwrap();
    let count2: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM backfill_progress WHERE season = 99991")
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);
    assert_eq!(count2, 2, "second seed must not duplicate rows");

    pucksdata::process::backfill::update_progress_status(pool, 9990000001, "done")
        .await
        .unwrap();
    pucksdata::process::backfill::update_progress_with_error(
        pool,
        9990000002,
        "failed",
        "synthetic failure",
    )
    .await
    .unwrap();
    pucksdata::process::backfill::seed_backfill_progress_with_refresh(pool, Some(99991), true)
        .await
        .unwrap();
    let refreshed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backfill_progress
         WHERE season = 99991 AND status = 'pending' AND error_message IS NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(refreshed, 2, "refresh must re-queue the complete season");

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99991")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9990000001, 9990000002)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99901, 99902)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_backfill_resume_skips_done() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99903, 'Resume Home', 'Home', 'Testville', 'RSH'),
                (99904, 'Resume Away', 'Away', 'Testville', 'RSA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000003, 99992, '2099-01-03', 99903, 99904, 2, 'OFF'),
                (9990000004, 99992, '2099-01-04', 99903, 99904, 2, 'OFF'),
                (9990000005, 99992, '2099-01-05', 99903, 99904, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99992))
        .await
        .unwrap();

    pucksdata::process::backfill::update_progress_status(pool, 9990000003, "done")
        .await
        .unwrap();
    pucksdata::process::backfill::update_progress_status(pool, 9990000004, "failed")
        .await
        .unwrap();

    let pending = pucksdata::process::backfill::query_pending_games(pool, Some(99992))
        .await
        .unwrap();
    let pending_ids: Vec<i64> = pending.iter().map(|g| g.game_id).collect();
    assert!(
        !pending_ids.contains(&9990000003),
        "done game must be excluded"
    );
    assert!(
        pending_ids.contains(&9990000004),
        "failed game must be included for retry"
    );
    assert!(
        pending_ids.contains(&9990000005),
        "pending game must be included"
    );
    assert_eq!(pending_ids.len(), 2, "exactly 2 non-done games expected");

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99992")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9990000003, 9990000004, 9990000005)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99903, 99904)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_backfill_status_transitions() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99905, 'Status Home', 'Home', 'Testville', 'STH'),
                (99906, 'Status Away', 'Away', 'Testville', 'STA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000006, 99993, '2099-01-06', 99905, 99906, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99993))
        .await
        .unwrap();

    let status1: String =
        sqlx::query_scalar!("SELECT status FROM backfill_progress WHERE game_id = 9990000006")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status1, "pending");

    pucksdata::process::backfill::update_progress_status(pool, 9990000006, "done")
        .await
        .unwrap();
    let status2: String =
        sqlx::query_scalar!("SELECT status FROM backfill_progress WHERE game_id = 9990000006")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status2, "done");

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99993")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id = 9990000006")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99905, 99906)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_query_pending_games_enriched() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99910, 'Enrich Home', 'EnHome', 'Testville', 'ENH'),
                (99911, 'Enrich Away', 'EnAway', 'Testville', 'ENA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000020, 99998, '2099-03-01', 99910, 99911, 2, 'OFF'),
                (9990000021, 99998, '2099-03-02', 99910, 99911, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99998))
        .await
        .unwrap();

    let pending = pucksdata::process::backfill::query_pending_games(pool, Some(99998))
        .await
        .unwrap();

    assert_eq!(pending.len(), 2, "should return exactly 2 non-done games");

    let first = &pending[0];
    assert_eq!(first.game_id, 9990000020);
    assert_eq!(first.season, 99998);
    assert_eq!(
        first.home_abbrev, "ENH",
        "home_abbrev must match inserted team"
    );
    assert_eq!(
        first.away_abbrev, "ENA",
        "away_abbrev must match inserted team"
    );
    assert!(
        first.game_date.year() > 0,
        "game_date year must be positive"
    );
    assert_eq!(first.game_date.year(), 2099, "game_date year must be 2099");

    let second = &pending[1];
    assert_eq!(second.game_id, 9990000021);
    assert_eq!(second.home_abbrev, "ENH");
    assert_eq!(second.away_abbrev, "ENA");

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99998")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9990000020, 9990000021)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99910, 99911)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_failed_game_records_error_message() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99920, 'Error Home', 'EHome', 'Testville', 'ERH'),
                (99921, 'Error Away', 'EAway', 'Testville', 'ERA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000030, 99996, '2099-04-01', 99920, 99921, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99996))
        .await
        .unwrap();

    pucksdata::process::backfill::update_progress_with_error(
        pool,
        9990000030,
        "failed",
        "HTTP error: 500",
    )
    .await
    .unwrap();

    let row = sqlx::query!(
        "SELECT status, error_message FROM backfill_progress WHERE game_id = 9990000030"
    )
    .fetch_one(pool)
    .await
    .unwrap();

    assert_eq!(row.status, "failed", "status must be 'failed'");
    assert_eq!(
        row.error_message.as_deref(),
        Some("HTTP error: 500"),
        "error_message must be 'HTTP error: 500'"
    );

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99996")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id = 9990000030")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99920, 99921)")
        .execute(pool)
        .await
        .unwrap();
}

#[test]
fn test_is_api_gap_error_unit() {
    let not_found: pucksdata::AnyError = Box::new(pucksdata::api::ApiError::NotFound);
    assert!(
        pucksdata::process::backfill::is_api_gap_error(&not_found),
        "ApiError::NotFound must classify as api gap error"
    );

    let server_err: pucksdata::AnyError = Box::new(pucksdata::api::ApiError::Other(500));
    assert!(
        !pucksdata::process::backfill::is_api_gap_error(&server_err),
        "ApiError::Other(500) must not classify as api gap error"
    );

    let io_err: pucksdata::AnyError = Box::new(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file not found",
    ));
    assert!(
        !pucksdata::process::backfill::is_api_gap_error(&io_err),
        "io::Error must not classify as api gap error"
    );
}

#[tokio::test]
async fn test_skipped_game_excluded_from_pending() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99922, 'Skip Home', 'SHome', 'Testville', 'SKH'),
                (99923, 'Skip Away', 'SAway', 'Testville', 'SKA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000031, 99997, '2099-05-01', 99922, 99923, 2, 'OFF'),
                (9990000032, 99997, '2099-05-02', 99922, 99923, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99997))
        .await
        .unwrap();

    pucksdata::process::backfill::update_progress_status(pool, 9990000031, "skipped")
        .await
        .unwrap();
    pucksdata::process::backfill::update_progress_status(pool, 9990000032, "failed")
        .await
        .unwrap();

    let pending = pucksdata::process::backfill::query_pending_games(pool, Some(99997))
        .await
        .unwrap();
    let pending_ids: Vec<i64> = pending.iter().map(|g| g.game_id).collect();

    assert!(
        !pending_ids.contains(&9990000031),
        "skipped game must be excluded from pending"
    );
    assert!(
        pending_ids.contains(&9990000032),
        "failed game must be included for retry"
    );
    assert_eq!(pending_ids.len(), 1, "exactly 1 non-terminal game expected");

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99997")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9990000031, 9990000032)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99922, 99923)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_checkpoint_kill_resume() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99930, 'Kill Home', 'KHome', 'Testville', 'KLH'),
                (99931, 'Kill Away', 'KAway', 'Testville', 'KLA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000040, 99999, '2099-06-01', 99930, 99931, 2, 'OFF'),
                (9990000041, 99999, '2099-06-02', 99930, 99931, 2, 'OFF'),
                (9990000042, 99999, '2099-06-03', 99930, 99931, 2, 'OFF'),
                (9990000043, 99999, '2099-06-04', 99930, 99931, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99999))
        .await
        .unwrap();

    // Simulate a run ending with done, skipped, failed, and in-flight games.
    pucksdata::process::backfill::update_progress_status(pool, 9990000040, "done")
        .await
        .unwrap();
    pucksdata::process::backfill::update_progress_status(pool, 9990000041, "skipped")
        .await
        .unwrap();
    pucksdata::process::backfill::update_progress_status(pool, 9990000042, "failed")
        .await
        .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99999))
        .await
        .unwrap();

    let pending = pucksdata::process::backfill::query_pending_games(pool, Some(99999))
        .await
        .unwrap();
    let pending_ids: Vec<i64> = pending.iter().map(|g| g.game_id).collect();

    assert!(
        !pending_ids.contains(&9990000040),
        "done game must be excluded after restart"
    );
    assert!(
        !pending_ids.contains(&9990000041),
        "skipped game must be excluded after restart"
    );
    assert!(
        pending_ids.contains(&9990000042),
        "failed game must be included for retry"
    );
    assert!(
        pending_ids.contains(&9990000043),
        "pending game must be included after restart"
    );
    assert_eq!(
        pending_ids.len(),
        2,
        "exactly 2 games should be pending after checkpoint resume"
    );

    sqlx::query!("DELETE FROM backfill_progress WHERE season = 99999")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!(
        "DELETE FROM games WHERE game_id IN (9990000040, 9990000041, 9990000042, 9990000043)"
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99930, 99931)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_backfill_season_scope() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99907, 'Scope Home', 'Home', 'Testville', 'SCH'),
                (99908, 'Scope Away', 'Away', 'Testville', 'SCA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9990000007, 99994, '2099-01-07', 99907, 99908, 2, 'OFF'),
                (9990000008, 99994, '2099-01-08', 99907, 99908, 2, 'OFF'),
                (9990000009, 99995, '2099-01-09', 99907, 99908, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    pucksdata::process::backfill::seed_backfill_progress(pool, Some(99994))
        .await
        .unwrap();

    let count_94: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM backfill_progress WHERE season = 99994")
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);
    assert_eq!(count_94, 2, "season 99994 should have 2 rows");

    let count_95: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM backfill_progress WHERE season = 99995")
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);
    assert_eq!(count_95, 0, "season 99995 should not be seeded");

    sqlx::query!("DELETE FROM backfill_progress WHERE season IN (99994, 99995)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9990000007, 9990000008, 9990000009)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99907, 99908)")
        .execute(pool)
        .await
        .unwrap();
}
