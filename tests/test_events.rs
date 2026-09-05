mod common;

#[test]
fn test_situation_code_decode() {
    use pucksdata::fetchers::events::decode_situation_code;

    let cases = [
        ("1551", true, 5, 5, true),
        ("1451", true, 4, 5, true),
        ("1541", true, 5, 4, true),
        ("0651", false, 6, 5, true),
        ("1560", true, 5, 6, false),
        ("1331", true, 3, 3, true),
        ("1341", true, 3, 4, true),
        ("0641", false, 6, 4, true),
    ];

    for (code, away_goalie, away_skaters, home_skaters, home_goalie) in cases {
        let situation = decode_situation_code(code).unwrap();
        assert_eq!(situation.away_goalie_present, away_goalie, "{code}");
        assert_eq!(situation.away_skater_count, away_skaters, "{code}");
        assert_eq!(situation.home_skater_count, home_skaters, "{code}");
        assert_eq!(situation.home_goalie_present, home_goalie, "{code}");
    }

    for malformed in ["", "155", "15511", "15x1", "2551", "1552"] {
        assert_eq!(decode_situation_code(malformed), None, "{malformed}");
    }
}

#[test]
fn test_strength_for_owner() {
    use pucksdata::fetchers::events::{decode_situation_code, strength_for_owner};

    let strength = |code, owner_is_home| {
        strength_for_owner(&decode_situation_code(code).unwrap(), owner_is_home)
    };

    assert_eq!(strength("1451", Some(true)), Some("pp"));
    assert_eq!(strength("1451", Some(false)), Some("sh"));
    assert_eq!(strength("0651", Some(true)), Some("ev"));
    assert_eq!(strength("0651", Some(false)), Some("ev"));
    assert_eq!(strength("0641", Some(false)), Some("pp"));
    assert_eq!(strength("0641", Some(true)), Some("sh"));
    assert_eq!(strength("1331", Some(true)), Some("ev"));
    assert_eq!(strength("1331", Some(false)), Some("ev"));
    assert_eq!(strength("1341", Some(true)), Some("pp"));
    assert_eq!(strength("1441", Some(true)), Some("ev"));
    assert_eq!(strength("1441", Some(false)), Some("ev"));
    assert_eq!(strength("1451", None), None);
}

#[test]
fn test_goal_details_deserialize() {
    use pucksdata::fetchers::events::EventDetails;

    let json = r#"{
        "xCoord": -56,
        "yCoord": 8,
        "zoneCode": "O",
        "shotType": "wrist",
        "scoringPlayerId": 8480801,
        "assist1PlayerId": 8478476,
        "assist2PlayerId": 8481533,
        "goalieInNetId": 8480382,
        "eventOwnerTeamId": 14
    }"#;
    let details: EventDetails = serde_json::from_str(json).unwrap();
    assert_eq!(details.scoring_player_id, Some(8480801_i64));
    assert_eq!(details.assist1_player_id, Some(8478476_i64));
    assert_eq!(details.assist2_player_id, Some(8481533_i64));
    assert_eq!(details.goalie_in_net_id, Some(8480382_i64));
    assert_eq!(details.shot_type.as_deref(), Some("wrist"));
    assert_eq!(details.x_coord, Some(-56_i16));
    assert_eq!(details.y_coord, Some(8_i16));
    assert_eq!(details.zone_code.as_deref(), Some("O"));

    let json_minimal = r#"{
        "xCoord": -56,
        "yCoord": 8,
        "zoneCode": "O",
        "shotType": "wrist",
        "scoringPlayerId": 8480801,
        "eventOwnerTeamId": 14
    }"#;
    let d2: EventDetails = serde_json::from_str(json_minimal).unwrap();
    assert_eq!(d2.assist1_player_id, None);
    assert_eq!(d2.assist2_player_id, None);
    assert_eq!(d2.goalie_in_net_id, None);

    let json_en = r#"{
        "xCoord": -70,
        "yCoord": 0,
        "zoneCode": "O",
        "shotType": "wrist",
        "scoringPlayerId": 8480801,
        "goalieInNetId": null,
        "eventOwnerTeamId": 14
    }"#;
    let d3: EventDetails = serde_json::from_str(json_en).unwrap();
    assert_eq!(
        d3.goalie_in_net_id, None,
        "null goalieInNetId should deserialize to None"
    );
}

#[test]
fn test_shot_details_deserialize() {
    use pucksdata::fetchers::events::EventDetails;

    let json = r#"{
        "xCoord": 65,
        "yCoord": -12,
        "zoneCode": "O",
        "shootingPlayerId": 8479318,
        "goalieInNetId": 8477293,
        "shotType": "slap"
    }"#;
    let details: EventDetails = serde_json::from_str(json).unwrap();
    assert_eq!(details.shooting_player_id, Some(8479318_i64));
    assert_eq!(details.goalie_in_net_id, Some(8477293_i64));
    assert_eq!(details.shot_type.as_deref(), Some("slap"));
    assert_eq!(details.x_coord, Some(65_i16));
    assert_eq!(details.y_coord, Some(-12_i16));
    assert_eq!(details.zone_code.as_deref(), Some("O"));
}

#[test]
fn test_hit_details_deserialize() {
    use pucksdata::fetchers::events::EventDetails;

    let json = r#"{
        "xCoord": 20,
        "yCoord": -30,
        "zoneCode": "N",
        "hittingPlayerId": 8478550,
        "hitteePlayerId": 8479355,
        "eventOwnerTeamId": 14
    }"#;
    let details: EventDetails = serde_json::from_str(json).unwrap();
    assert_eq!(details.hitting_player_id, Some(8478550_i64));
    assert_eq!(details.hittee_player_id, Some(8479355_i64));
    assert_eq!(details.x_coord, Some(20_i16));
    assert_eq!(details.y_coord, Some(-30_i16));
    assert_eq!(details.zone_code.as_deref(), Some("N"));
}

#[test]
fn test_block_details_deserialize() {
    use pucksdata::fetchers::events::EventDetails;

    let json = r#"{
        "xCoord": 55,
        "yCoord": 10,
        "zoneCode": "D",
        "blockingPlayerId": 8476412,
        "shootingPlayerId": 8481533,
        "eventOwnerTeamId": 18
    }"#;
    let details: EventDetails = serde_json::from_str(json).unwrap();
    assert_eq!(details.blocking_player_id, Some(8476412_i64));
    assert_eq!(details.shooting_player_id, Some(8481533_i64));
    assert_eq!(details.x_coord, Some(55_i16));
    assert_eq!(details.y_coord, Some(10_i16));
    assert_eq!(details.zone_code.as_deref(), Some("D"));
}

#[test]
fn test_penalty_details_deserialize() {
    use pucksdata::fetchers::events::EventDetails;

    // Penalties use `duration` and `descKey`, unlike several related NHL payloads.
    let json = r#"{
        "xCoord": 10,
        "yCoord": -20,
        "zoneCode": "N",
        "typeCode": "MIN",
        "descKey": "high-sticking",
        "duration": 2,
        "committedByPlayerId": 8479318,
        "drawnByPlayerId": 8480801,
        "eventOwnerTeamId": 18
    }"#;
    let details: EventDetails = serde_json::from_str(json).unwrap();
    assert_eq!(
        details.duration,
        Some(2_i16),
        "duration should be 2 (integer minutes)"
    );
    assert_eq!(
        details.desc_key.as_deref(),
        Some("high-sticking"),
        "descKey → desc_key"
    );
    assert_eq!(details.type_code.as_deref(), Some("MIN"));
    assert_eq!(details.committed_by_player_id, Some(8479318_i64));
    assert_eq!(details.drawn_by_player_id, Some(8480801_i64));

    let json_bench = r#"{
        "typeCode": "MIN",
        "descKey": "too-many-men",
        "duration": 2,
        "committedByPlayerId": null,
        "eventOwnerTeamId": 18
    }"#;
    let d2: EventDetails = serde_json::from_str(json_bench).unwrap();
    assert_eq!(
        d2.drawn_by_player_id, None,
        "bench minor: drawnByPlayerId should be None"
    );
    assert_eq!(
        d2.committed_by_player_id, None,
        "bench minor: committedByPlayerId null → None"
    );
}

#[test]
fn test_faceoff_details_deserialize() {
    use pucksdata::fetchers::events::EventDetails;

    let json = r#"{
        "xCoord": 0,
        "yCoord": 0,
        "zoneCode": "N",
        "winningPlayerId": 8476346,
        "losingPlayerId": 8479323,
        "eventOwnerTeamId": 14
    }"#;
    let details: EventDetails = serde_json::from_str(json).unwrap();
    assert_eq!(details.winning_player_id, Some(8476346_i64));
    assert_eq!(details.losing_player_id, Some(8479323_i64));
    assert_eq!(details.x_coord, Some(0_i16));
    assert_eq!(details.y_coord, Some(0_i16));
    assert_eq!(details.zone_code.as_deref(), Some("N"));
}

// Goal-derived shot idempotency.

#[test]
fn test_goal_produces_shot_entry() {
    use pucksdata::fetchers::events::{
        transform_events, EventDetails, PbpTeam, PeriodDescriptor, Play, PlayByPlay,
    };
    use std::collections::HashMap;

    let pbp = PlayByPlay {
        id: 2025020004,
        home_team: PbpTeam {
            id: 10,
            abbrev: None,
        },
        away_team: PbpTeam {
            id: 8,
            abbrev: None,
        },
        plays: vec![
            Play {
                event_id: 1081,
                period_descriptor: PeriodDescriptor {
                    number: 3,
                    period_type: "REG".to_string(),
                },
                time_in_period: "18:28".to_string(),
                situation_code: Some("0651".to_string()),
                type_desc_key: "goal".to_string(),
                details: Some(EventDetails {
                    x_coord: Some(-87),
                    y_coord: Some(-6),
                    zone_code: Some("O".to_string()),
                    event_owner_team_id: Some(10),
                    scoring_player_id: Some(8479318),
                    assist1_player_id: Some(8477939),
                    assist2_player_id: None,
                    goalie_in_net_id: None,
                    shot_type: Some("wrist".to_string()),
                    shooting_player_id: None,
                    hitting_player_id: None,
                    hittee_player_id: None,
                    blocking_player_id: None,
                    type_code: None,
                    desc_key: None,
                    duration: None,
                    committed_by_player_id: None,
                    drawn_by_player_id: None,
                    winning_player_id: None,
                    losing_player_id: None,
                }),
            },
            Play {
                event_id: 200,
                period_descriptor: PeriodDescriptor {
                    number: 1,
                    period_type: "REG".to_string(),
                },
                time_in_period: "15:00".to_string(),
                situation_code: Some("1551".to_string()),
                type_desc_key: "shot-on-goal".to_string(),
                details: Some(EventDetails {
                    x_coord: Some(65),
                    y_coord: Some(-12),
                    zone_code: Some("O".to_string()),
                    event_owner_team_id: Some(8),
                    scoring_player_id: None,
                    assist1_player_id: None,
                    assist2_player_id: None,
                    goalie_in_net_id: Some(8480382),
                    shot_type: Some("slap".to_string()),
                    shooting_player_id: Some(8479318),
                    hitting_player_id: None,
                    hittee_player_id: None,
                    blocking_player_id: None,
                    type_code: None,
                    desc_key: None,
                    duration: None,
                    committed_by_player_id: None,
                    drawn_by_player_id: None,
                    winning_player_id: None,
                    losing_player_id: None,
                }),
            },
        ],
    };

    let team_id_map = HashMap::new();
    let (events, goals, shots, _hits, _blocks, _penalties, _faceoffs, warnings) =
        transform_events(&pbp, &team_id_map);

    assert!(
        warnings.is_empty(),
        "no skip warnings expected: {:?}",
        warnings
    );
    assert_eq!(goals.len(), 1, "expected 1 goal");
    assert_eq!(
        shots.len(),
        2,
        "expected 2 shots (goal-derived + shot-on-goal)"
    );

    let goal_event = events
        .iter()
        .find(|event| event.event_id_in_game == 1081)
        .expect("goal event must be present");
    assert_eq!(goal_event.away_goalie_present, Some(false));
    assert_eq!(goal_event.away_skater_count, Some(6));
    assert_eq!(goal_event.home_skater_count, Some(5));
    assert_eq!(goal_event.home_goalie_present, Some(true));
    assert_eq!(goal_event.strength.as_deref(), Some("ev"));
    assert_eq!(
        goal_event.strength_source,
        pucksdata::models::StrengthSource::SituationCode
    );
    assert_eq!(goal_event.situation_code.as_deref(), Some("0651"));

    let goal_shot = shots
        .iter()
        .find(|s| s.event_id_in_game == 1081)
        .expect("goal-derived shot must have the goal event ID");
    assert_eq!(
        goal_shot.shooting_player_id,
        Some(8479318),
        "goal scorer maps to shooting_player_id"
    );
    assert_eq!(
        goal_shot.goalie_in_net_id, None,
        "empty-net goal has no goalie_in_net_id"
    );
    assert_eq!(
        goal_shot.shot_type.as_deref(),
        Some("wrist"),
        "shot_type carried through from goal event"
    );

    let reg_shot = shots
        .iter()
        .find(|s| s.event_id_in_game == 200)
        .expect("regular shot-on-goal must be present");
    assert_eq!(reg_shot.shooting_player_id, Some(8479318));
}

#[test]
fn test_missing_situation_uses_goal_summary_without_fabricating_on_ice_state() {
    use pucksdata::fetchers::events::{
        transform_events, transform_events_with_goal_strengths,
        transform_events_with_strength_sources, EventStrength, PlayByPlay,
    };
    use pucksdata::models::StrengthSource;
    use std::collections::HashMap;

    let pbp: PlayByPlay = serde_json::from_str(
        r#"{
            "id": 2005020001,
            "homeTeam": {"id": 6},
            "awayTeam": {"id": 8},
            "plays": [{
                "eventId": 10088563,
                "periodDescriptor": {"number": 3, "periodType": "REG"},
                "timeInPeriod": "19:48",
                "typeDescKey": "goal",
                "details": {
                    "eventOwnerTeamId": 8,
                    "scoringPlayerId": 8467545,
                    "shotType": "slap"
                }
            }]
        }"#,
    )
    .unwrap();
    let teams = HashMap::from([(6, 6), (8, 8)]);

    let (events, ..) = transform_events(&pbp, &teams);
    let event = &events[0];
    assert_eq!(event.strength, None);
    assert_eq!(event.strength_source, StrengthSource::Unavailable);
    assert_eq!(event.situation_code, None);
    assert_eq!(event.away_goalie_present, None);
    assert_eq!(event.away_skater_count, None);
    assert_eq!(event.home_skater_count, None);
    assert_eq!(event.home_goalie_present, None);

    let strengths = HashMap::from([(10088563, EventStrength::PowerPlay)]);
    let (events, ..) = transform_events_with_goal_strengths(&pbp, &teams, &strengths);
    let event = &events[0];
    assert_eq!(event.strength.as_deref(), Some("pp"));
    assert_eq!(event.strength_source, StrengthSource::ScoringSummary);
    assert_eq!(event.situation_code, None);
    assert_eq!(event.away_goalie_present, None);
    assert_eq!(event.away_skater_count, None);
    assert_eq!(event.home_skater_count, None);
    assert_eq!(event.home_goalie_present, None);

    let report_strengths = HashMap::from([(10088563, EventStrength::ShortHanded)]);
    let (events, ..) =
        transform_events_with_strength_sources(&pbp, &teams, &HashMap::new(), &report_strengths);
    let event = &events[0];
    assert_eq!(event.strength.as_deref(), Some("sh"));
    assert_eq!(event.strength_source, StrengthSource::HtmlReport);
}

// Database integration tests.

#[tokio::test]
async fn test_events_upsert_idempotent() {
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

    sqlx::query!(
        "INSERT INTO games (game_id, season, game_date, home_team_id, away_team_id, game_type)
         VALUES (9900000002, 20232024, '2024-01-01', 99001, 99002, 2)
         ON CONFLICT (game_id) DO NOTHING"
    )
    .execute(pool)
    .await
    .unwrap();

    let event = pucksdata::models::DbEvent {
        game_id: 9900000002,
        event_id_in_game: 1,
        period: 1,
        period_type: "REG".into(),
        time_in_period: "05:00".into(),
        event_type: "goal".into(),
        x_coord: None,
        y_coord: None,
        zone_code: None,
        event_owner_team_id: Some(99001),
        home_goalie_present: Some(true),
        home_skater_count: Some(5),
        away_skater_count: Some(5),
        away_goalie_present: Some(true),
        strength: Some("ev".into()),
        strength_source: pucksdata::models::StrengthSource::SituationCode,
        situation_code: Some("1551".into()),
    };
    let goal = pucksdata::models::DbGoal {
        event_id_in_game: 1,
        scorer_player_id: None,
        assist1_player_id: None,
        assist2_player_id: None,
        goalie_id: None,
        shot_type: None,
    };
    let stale_event = pucksdata::models::DbEvent {
        game_id: 9900000002,
        event_id_in_game: 2,
        period: 1,
        period_type: "REG".into(),
        time_in_period: "06:00".into(),
        event_type: "shot-on-goal".into(),
        x_coord: None,
        y_coord: None,
        zone_code: None,
        event_owner_team_id: Some(99002),
        home_goalie_present: Some(true),
        home_skater_count: Some(5),
        away_skater_count: Some(5),
        away_goalie_present: Some(true),
        strength: Some("ev".into()),
        strength_source: pucksdata::models::StrengthSource::SituationCode,
        situation_code: Some("1551".into()),
    };

    // First snapshot contains an event later removed by an NHL feed revision.
    pucksdata::loaders::events::upsert_game_events(
        pool,
        9900000002,
        &[event, stale_event],
        &[goal],
        &[],
        &[],
        &[],
        &[],
        &[],
    )
    .await
    .unwrap();

    let event2 = pucksdata::models::DbEvent {
        game_id: 9900000002,
        event_id_in_game: 1,
        period: 1,
        period_type: "REG".into(),
        time_in_period: "05:00".into(),
        event_type: "goal".into(),
        x_coord: None,
        y_coord: None,
        zone_code: None,
        event_owner_team_id: Some(99001),
        home_goalie_present: Some(true),
        home_skater_count: Some(5),
        away_skater_count: Some(4),
        away_goalie_present: Some(true),
        strength: Some("pp".into()),
        strength_source: pucksdata::models::StrengthSource::SituationCode,
        situation_code: Some("1451".into()),
    };
    let goal2 = pucksdata::models::DbGoal {
        event_id_in_game: 1,
        scorer_player_id: None,
        assist1_player_id: None,
        assist2_player_id: None,
        goalie_id: None,
        shot_type: None,
    };

    // Second snapshot must replace the first and remove the stale event.
    pucksdata::loaders::events::upsert_game_events(
        pool,
        9900000002,
        &[event2],
        &[goal2],
        &[],
        &[],
        &[],
        &[],
        &[],
    )
    .await
    .unwrap();

    let event_count: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM events WHERE game_id = 9900000002")
            .fetch_one(pool)
            .await
            .unwrap()
            .unwrap_or(0);
    assert_eq!(event_count, 1, "upsert produced more than one event row");

    let goal_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM goals g JOIN events e ON e.id = g.event_id WHERE e.game_id = 9900000002"
    ).fetch_one(pool).await.unwrap().unwrap_or(0);
    assert_eq!(goal_count, 1, "upsert produced more than one goal row");

    let (strength, strength_source, situation_code, away_skaters): (
        Option<String>,
        String,
        Option<String>,
        Option<i16>,
    ) = sqlx::query_as(
        "SELECT strength, strength_source, situation_code, away_skater_count
             FROM events WHERE game_id = 9900000002 AND event_id_in_game = 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(strength.as_deref(), Some("pp"));
    assert_eq!(strength_source, "situation_code");
    assert_eq!(situation_code.as_deref(), Some("1451"));
    assert_eq!(away_skaters, Some(4));

    sqlx::query!(
        "DELETE FROM goals WHERE event_id IN (SELECT id FROM events WHERE game_id = 9900000002)"
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!("DELETE FROM events WHERE game_id = 9900000002")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM games WHERE game_id = 9900000002")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query!("DELETE FROM teams WHERE team_id IN (99001, 99002)")
        .execute(pool)
        .await
        .unwrap();
}
