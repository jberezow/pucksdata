#[tokio::test]
async fn test_teams_upsert_idempotent() {
    if !common::test_database_configured() {
        return;
    }
    let pool = common::test_pool().await;
    let record = pucksdata::models::DbTeam {
        team_id: 999999,
        full_name: "Test Team".into(),
        common_name: "Tests".into(),
        place_name: "Testville".into(),
        abbrev: "TST".into(),
    };
    pucksdata::loaders::teams::upsert_teams(pool, &[record], &indicatif::ProgressBar::hidden())
        .await
        .unwrap();
    pucksdata::loaders::teams::upsert_teams(
        pool,
        &[pucksdata::models::DbTeam {
            team_id: 999999,
            full_name: "Test Team Updated".into(),
            common_name: "Tests".into(),
            place_name: "Testville".into(),
            abbrev: "TST".into(),
        }],
        &indicatif::ProgressBar::hidden(),
    )
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM teams WHERE team_id = 999999")
        .fetch_one(pool)
        .await
        .unwrap()
        .unwrap_or(0);
    assert_eq!(count, 1, "upsert produced more than one row");
    let name: String = sqlx::query_scalar!("SELECT full_name FROM teams WHERE team_id = 999999")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(name, "Test Team Updated");
    sqlx::query!("DELETE FROM teams WHERE team_id = 999999")
        .execute(pool)
        .await
        .unwrap();
}
mod common;
