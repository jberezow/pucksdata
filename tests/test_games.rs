#[test]
fn test_games_deserialize_stats_response() {
    // The stats endpoint uses `visitingTeamId` and `visitingScore`.
    let json = r#"{
        "data": [
            {
                "id": 2024020001,
                "season": 20242025,
                "gameDate": "2024-10-08",
                "gameType": 2,
                "homeTeamId": 10,
                "visitingTeamId": 22,
                "homeScore": 3,
                "visitingScore": 1
            },
            {
                "id": 2024030211,
                "season": 20242025,
                "gameDate": "2025-05-01",
                "gameType": 3,
                "homeTeamId": 6,
                "visitingTeamId": 17,
                "homeScore": null,
                "visitingScore": null
            }
        ],
        "total": 2
    }"#;

    use pucksdata::fetchers::games::{StatsApiResponse, StatsGameRecord};
    let resp: StatsApiResponse<StatsGameRecord> = serde_json::from_str(json).unwrap();
    assert_eq!(resp.data.len(), 2);

    let g = &resp.data[0];
    assert_eq!(g.id, 2024020001_i64);
    assert_eq!(g.season, 20242025_i32);
    assert_eq!(g.away_team_id, 22_i64); // visitingTeamId mapped to away_team_id
    assert_eq!(g.home_score, Some(3_i16));
    assert_eq!(g.away_score, Some(1_i16)); // visitingScore mapped to away_score

    // Playoff game ID exceeds i32 max — must be i64
    let playoff = &resp.data[1];
    assert_eq!(playoff.id, 2024030211_i64);
    assert!(playoff.home_score.is_none());

    let boxscore_json = r#"{
        "id": 2024020001,
        "startTimeUTC": "2024-10-09T00:00:00Z",
        "gameState": "OFF",
        "venue": {"default": "United Center"},
        "venueLocation": {"default": "Chicago, IL"},
        "homeTeam": {"id": 10, "score": 3},
        "awayTeam": {"id": 22, "score": 1}
    }"#;

    use pucksdata::fetchers::games::BoxscoreGame;
    let bs: BoxscoreGame = serde_json::from_str(boxscore_json).unwrap();
    assert_eq!(bs.id, 2024020001_i64);
    assert_eq!(
        bs.venue.as_ref().map(|v| v.default.as_str()),
        Some("United Center")
    );
    assert_eq!(bs.home_team.score, Some(3_i16));
    assert_eq!(bs.away_team.score, Some(1_i16));
}

#[tokio::test]
async fn test_games_upsert_idempotent() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    sqlx::query!(
        "INSERT INTO teams (team_id, full_name, common_name, place_name, abbrev)
         VALUES (99001, 'Test Home', 'Home', 'Testville', 'HME'),
                (99002, 'Test Away', 'Away', 'Testville', 'AWY')
         ON CONFLICT (team_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    let game_date = time::macros::date!(2024 - 10 - 08);
    let record = pucksdata::models::DbGame {
        game_id: 9900000001_i64,
        season: 20242025,
        game_date,
        start_time_utc: None,
        home_team_id: 99001,
        away_team_id: 99002,
        game_type: 2,
        venue: Some("Test Arena".into()),
        venue_location: Some("Testville, TS".into()),
        game_state: Some("OFF".into()),
        home_score: Some(3),
        away_score: Some(1),
    };

    pucksdata::loaders::games::upsert_games(pool, &[record], &indicatif::ProgressBar::hidden())
        .await
        .unwrap();

    let record2 = pucksdata::models::DbGame {
        game_id: 9900000001_i64,
        season: 20242025,
        game_date,
        start_time_utc: None,
        home_team_id: 99001,
        away_team_id: 99002,
        game_type: 2,
        venue: Some("Test Arena Updated".into()),
        venue_location: Some("Testville, TS".into()),
        game_state: Some("OFF".into()),
        home_score: Some(3),
        away_score: Some(1),
    };
    pucksdata::loaders::games::upsert_games(pool, &[record2], &indicatif::ProgressBar::hidden())
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM games WHERE game_id = 9900000001")
        .fetch_one(pool)
        .await
        .unwrap()
        .unwrap_or(0);
    assert_eq!(count, 1, "upsert produced more than one row");

    let venue: Option<String> =
        sqlx::query_scalar!("SELECT venue FROM games WHERE game_id = 9900000001")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(venue.as_deref(), Some("Test Arena Updated"));

    sqlx::query!("DELETE FROM games WHERE game_id = 9900000001")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99001, 99002)")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore]
async fn test_fetch_idempotency() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;

    // This live test expects the 2024–25 teams to be loaded already.
    let test_season = 20242025_i32;

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    let games_run1 =
        pucksdata::fetchers::games::fetch_games_for_season_enriched(test_season, &pb).await;
    assert!(
        !games_run1.is_empty(),
        "expected at least one game for season {test_season}"
    );
    pucksdata::loaders::games::upsert_games(pool, &games_run1, &pb)
        .await
        .unwrap();

    let count_after_run1: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM games WHERE season = $1", test_season)
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);

    let games_run2 =
        pucksdata::fetchers::games::fetch_games_for_season_enriched(test_season, &pb).await;
    pucksdata::loaders::games::upsert_games(pool, &games_run2, &pb)
        .await
        .unwrap();

    let count_after_run2: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM games WHERE season = $1", test_season)
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);

    assert_eq!(
        count_after_run1, count_after_run2,
        "Re-running games fetch for season {test_season} changed the row count: \
         run1={count_after_run1}, run2={count_after_run2}. Upsert semantics violated."
    );

    pb.finish_with_message(format!(
        "Idempotency verified: {count_after_run2} games for season {test_season} after 2 runs"
    ));
}
mod common;
