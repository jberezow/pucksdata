#[test]
fn test_player_landing_deserialize() {
    let json = r#"{
        "playerId": 8478402,
        "firstName": {"default": "Connor"},
        "lastName": {"default": "McDavid"},
        "position": "C",
        "shootsCatches": "L",
        "currentTeamAbbrev": "EDM",
        "birthDate": "1997-01-13",
        "heightInCentimeters": 185,
        "weightInKilograms": 88,
        "draftDetails": {"year": 2015, "teamAbbrev": "EDM", "round": 1, "pickInRound": 1, "overallPick": 1}
    }"#;
    let player: pucksdata::fetchers::players::PlayerLanding = serde_json::from_str(json).unwrap();
    assert_eq!(player.first_name.default, "Connor");
    assert_eq!(player.last_name.default, "McDavid");
    assert!(player.draft_details.is_some());

    let json2 = r#"{
        "playerId": 9999999,
        "firstName": {"default": "Test"},
        "lastName": {"default": "Player"},
        "position": null,
        "shootsCatches": null
    }"#;
    let p2: pucksdata::fetchers::players::PlayerLanding = serde_json::from_str(json2).unwrap();
    assert_eq!(p2.first_name.default, "Test");
    assert!(p2.current_team_abbrev.is_none());
    assert!(p2.draft_details.is_none());
}

#[tokio::test]
async fn test_players_upsert_idempotent() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;
    let record = pucksdata::models::DbPlayer {
        player_id: 9000001,
        first_name: "Test".into(),
        last_name: "Player".into(),
        position: Some("C".into()),
        shoots_catches: Some("L".into()),
        current_team_abbrev: None,
        birth_date: None,
        height_cm: Some(185),
        weight_kg: Some(90),
        draft_year: None,
        draft_round: None,
        draft_pick: None,
        draft_team_abbrev: None,
        draft_overall_pick: None,
    };
    pucksdata::loaders::players::upsert_players(pool, &[record])
        .await
        .unwrap();
    pucksdata::loaders::players::upsert_players(
        pool,
        &[pucksdata::models::DbPlayer {
            player_id: 9000001,
            first_name: "Test".into(),
            last_name: "Player Updated".into(),
            position: Some("C".into()),
            shoots_catches: Some("L".into()),
            current_team_abbrev: None,
            birth_date: None,
            height_cm: Some(185),
            weight_kg: Some(90),
            draft_year: None,
            draft_round: None,
            draft_pick: None,
            draft_team_abbrev: None,
            draft_overall_pick: None,
        }],
    )
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM players WHERE player_id = 9000001")
        .fetch_one(pool)
        .await
        .unwrap()
        .unwrap_or(0);
    assert_eq!(count, 1, "upsert produced more than one row");
    let name: String =
        sqlx::query_scalar!("SELECT last_name FROM players WHERE player_id = 9000001")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(name, "Player Updated");
    sqlx::query!("DELETE FROM players WHERE player_id = 9000001")
        .execute(pool)
        .await
        .unwrap();
}
mod common;
