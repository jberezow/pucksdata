#[test]
fn test_coverage_calculation_unit() {
    let total_games = 5_i64;
    let covered_games = 5_i64;
    let pct = if total_games > 0 {
        (covered_games as f64 / total_games as f64) * 100.0
    } else {
        100.0
    };
    assert!(
        (pct - 100.0).abs() < 0.01,
        "100% coverage expected when games_with_events == total_off_games"
    );

    let covered_games = 4_i64;
    let pct2 = if total_games > 0 {
        (covered_games as f64 / total_games as f64) * 100.0
    } else {
        100.0
    };
    assert!((pct2 - 80.0).abs() < 0.01, "80% coverage expected for 4/5");

    let total_games = 0_i64;
    let covered_games = 0_i64;
    let pct3 = if total_games > 0 {
        (covered_games as f64 / total_games as f64) * 100.0
    } else {
        100.0
    };
    assert!(
        (pct3 - 100.0).abs() < 0.01,
        "0 off games should report 100% (trivially healthy)"
    );
}

#[tokio::test]
async fn test_status_query_healthy_season() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99941, 'Status Home A', 'StatHA', 'Testville', 'SHA'),
                (99942, 'Status Away A', 'StatAA', 'Testville', 'SAA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9992000001, 99981, '2099-01-01', 99941, 99942, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    sqlx::query!(
        "INSERT INTO events (game_id, event_id_in_game, period, period_type, time_in_period, event_type)
         VALUES (9992000001, 1, 1, 'REG', '00:00', 'goal')
         ON CONFLICT (game_id, event_id_in_game) DO NOTHING"
    ).execute(pool).await.unwrap();

    let healthy = pucksdata::process::status::run_status(pool, Some(99981), false)
        .await
        .unwrap();

    assert!(
        healthy,
        "season with all OFF games covered must return healthy=true"
    );

    let report = pucksdata::process::status::collect_health(pool, Some(99981))
        .await
        .unwrap();
    assert_eq!(report.seasons.len(), 1);
    assert_eq!(report.seasons[0].completed_games, 1);
    assert_eq!(report.seasons[0].missing_event_games, 0);
    assert_eq!(report.seasons[0].acknowledged_gap_games, 0);
    assert_eq!(report.seasons[0].actionable_gap_games, 0);
    assert!(report.seasons[0].healthy);

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["seasons"][0]["season"], 99981);
    assert!(json["summary"].get("last_sync_at").is_some());

    sqlx::query!("DELETE FROM events WHERE game_id = 9992000001")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id = 9992000001")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99941, 99942)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_status_query_unhealthy_season() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99943, 'Status Home B', 'StatHB', 'Testville', 'SHB'),
                (99944, 'Status Away B', 'StatAB', 'Testville', 'SAB')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9992000002, 99982, '2099-01-02', 99943, 99944, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    let healthy = pucksdata::process::status::run_status(pool, Some(99982), false)
        .await
        .unwrap();

    assert!(
        !healthy,
        "season with uncovered OFF game must return healthy=false"
    );

    sqlx::query!("DELETE FROM games WHERE game_id = 9992000002")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99943, 99944)")
        .execute(pool)
        .await
        .unwrap();
}

/// A completed game explicitly classified as done remains a visible gap but is not actionable.
#[tokio::test]
async fn test_status_classifies_acknowledged_gap() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99953, 'Known Gap Home', 'KnownH', 'Testville', 'KGH'),
                (99954, 'Known Gap Away', 'KnownA', 'Testville', 'KGA')
         ON CONFLICT (team_id) DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9992000011, 99987, '2099-02-02', 99953, 99954, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO backfill_progress (game_id, season, status)
         VALUES (9992000011, 99987, 'done')
         ON CONFLICT (game_id) DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();

    let healthy = pucksdata::process::status::run_status(pool, Some(99987), true)
        .await
        .unwrap();
    assert!(!healthy, "strict health must retain known gaps");

    let report = pucksdata::process::status::collect_health(pool, Some(99987))
        .await
        .unwrap();
    let season = &report.seasons[0];
    assert_eq!(season.missing_event_games, 1);
    assert_eq!(season.acknowledged_gap_games, 1);
    assert_eq!(season.actionable_gap_games, 0);
    assert!(!season.healthy);

    let status: String =
        sqlx::query_scalar("SELECT status FROM backfill_progress WHERE game_id = 9992000011")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "done", "--fix must not requeue acknowledged gaps");

    sqlx::query("DELETE FROM backfill_progress WHERE game_id = 9992000011")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM games WHERE game_id = 9992000011")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM teams WHERE team_id IN (99953, 99954)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_status_season_filter() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99945, 'Status Home C', 'StatHC', 'Testville', 'SHC'),
                (99946, 'Status Away C', 'StatAC', 'Testville', 'SAC')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9992000003, 99983, '2099-01-03', 99945, 99946, 2, 'OFF'),
                (9992000004, 99984, '2099-01-04', 99945, 99946, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    sqlx::query!(
        "INSERT INTO events (game_id, event_id_in_game, period, period_type, time_in_period, event_type)
         VALUES (9992000003, 1, 1, 'REG', '00:00', 'goal')
         ON CONFLICT (game_id, event_id_in_game) DO NOTHING"
    ).execute(pool).await.unwrap();

    let healthy_scoped = pucksdata::process::status::run_status(pool, Some(99983), false)
        .await
        .unwrap();
    assert!(
        healthy_scoped,
        "season-scoped query must only see season 99983, which is healthy"
    );

    let unhealthy_scoped = pucksdata::process::status::run_status(pool, Some(99984), false)
        .await
        .unwrap();
    assert!(
        !unhealthy_scoped,
        "season-scoped query for 99984 must return unhealthy (game has no events)"
    );

    sqlx::query!("DELETE FROM events WHERE game_id = 9992000003")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9992000003, 9992000004)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99945, 99946)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_status_excludes_fut_pre_games() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99947, 'Status Home D', 'StatHD', 'Testville', 'SHD'),
                (99948, 'Status Away D', 'StatAD', 'Testville', 'SAD')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9992000005, 99985, '2099-01-05', 99947, 99948, 2, 'OFF'),
                (9992000006, 99985, '2099-09-01', 99947, 99948, 2, 'FUT')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    sqlx::query!(
        "INSERT INTO events (game_id, event_id_in_game, period, period_type, time_in_period, event_type)
         VALUES (9992000005, 1, 1, 'REG', '00:00', 'goal')
         ON CONFLICT (game_id, event_id_in_game) DO NOTHING"
    ).execute(pool).await.unwrap();

    let healthy = pucksdata::process::status::run_status(pool, Some(99985), false)
        .await
        .unwrap();
    assert!(
        healthy,
        "FUT/PRE games must not count toward total_off_games (season should be healthy)"
    );

    sqlx::query!("DELETE FROM events WHERE game_id = 9992000005")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id IN (9992000005, 9992000006)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99947, 99948)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_fix_idempotent() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99951, 'Fix Home', 'FixH', 'Testville', 'FXH'),
                (99952, 'Fix Away', 'FixA', 'Testville', 'FXA')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type, game_state)
         VALUES (9992000010, 99986, '2099-02-01', 99951, 99952, 2, 'OFF')
         ON CONFLICT (game_id) DO NOTHING"
    ).execute(pool).await.unwrap();

    sqlx::query!(
        "INSERT INTO events (game_id, event_id_in_game, period, period_type, time_in_period, event_type)
         VALUES (9992000010, 1, 1, 'REG', '00:00', 'goal')
         ON CONFLICT (game_id, event_id_in_game) DO NOTHING"
    ).execute(pool).await.unwrap();

    sqlx::query!(
        "INSERT INTO backfill_progress (game_id, season, status)
         VALUES (9992000010, 99986, 'done')
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    let healthy = pucksdata::process::status::run_status(pool, Some(99986), true)
        .await
        .unwrap();

    assert!(
        healthy,
        "already-healthy season with fix=true must still return healthy=true"
    );

    let bp_status: Option<String> =
        sqlx::query_scalar!("SELECT status FROM backfill_progress WHERE game_id = 9992000010")
            .fetch_optional(pool)
            .await
            .unwrap();
    assert_eq!(
        bp_status.as_deref(),
        Some("done"),
        "backfill_progress must remain 'done' after no-op fix"
    );

    sqlx::query!("DELETE FROM backfill_progress WHERE game_id = 9992000010")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM events WHERE game_id = 9992000010")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id = 9992000010")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99951, 99952)")
        .execute(pool)
        .await
        .unwrap();
}

mod common;
