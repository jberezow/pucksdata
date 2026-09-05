//! Upserts game records to the `games` table.
use crate::models::DbGame;

/// Convert a chrono::DateTime<Utc> to time::OffsetDateTime.
///
/// sqlx 0.8 with the `time` feature maps TIMESTAMPTZ to time::OffsetDateTime at the macro level.
fn chrono_to_time(dt: chrono::DateTime<chrono::Utc>) -> time::OffsetDateTime {
    let ts = dt.timestamp();
    let nanos = dt.timestamp_subsec_nanos();
    time::OffsetDateTime::from_unix_timestamp_nanos((ts as i128) * 1_000_000_000 + nanos as i128)
        .expect("valid timestamp from chrono")
}

/// Upsert a batch of games into the games table.
///
/// Uses `ON CONFLICT (game_id) DO UPDATE` for idempotency.
/// Returns the count of records processed.
///
/// FK note: home_team_id and away_team_id reference teams(team_id).
/// The caller is responsible for ensuring teams exist before loading games.
/// FK violations surface as sqlx errors and propagate to the caller.
///
/// `pb` is the upsert-phase progress bar. Call `pb.finish_and_clear()` after this
/// returns. Use `ProgressBar::hidden()` for callers that don't need a visible bar.
pub async fn upsert_games(
    pool: &sqlx::PgPool,
    records: &[DbGame],
    pb: &indicatif::ProgressBar,
) -> Result<usize, sqlx::Error> {
    for g in records {
        // sqlx 0.8 `time` feature maps TIMESTAMPTZ -> time::OffsetDateTime.
        // The model stores chrono::DateTime<Utc>, so convert at bind time.
        let start_time_utc: Option<time::OffsetDateTime> = g.start_time_utc.map(chrono_to_time);

        sqlx::query!(
            r#"
            INSERT INTO games
                (game_id, season, game_date, start_time_utc, home_team_id, away_team_id,
                 game_type, venue, venue_location, game_state, home_score, away_score)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (game_id) DO UPDATE SET
                season         = EXCLUDED.season,
                game_date      = EXCLUDED.game_date,
                start_time_utc = EXCLUDED.start_time_utc,
                home_team_id   = EXCLUDED.home_team_id,
                away_team_id   = EXCLUDED.away_team_id,
                game_type      = EXCLUDED.game_type,
                venue          = EXCLUDED.venue,
                venue_location = EXCLUDED.venue_location,
                game_state     = EXCLUDED.game_state,
                home_score     = EXCLUDED.home_score,
                away_score     = EXCLUDED.away_score
            "#,
            g.game_id,
            g.season,
            g.game_date,
            start_time_utc,
            g.home_team_id,
            g.away_team_id,
            g.game_type,
            g.venue.as_deref(),
            g.venue_location.as_deref(),
            g.game_state.as_deref(),
            g.home_score,
            g.away_score,
        )
        .execute(pool)
        .await?;
        pb.suspend(|| println!("{}  game {}", g.game_date, g.game_id));
        pb.inc(1);
    }
    Ok(records.len())
}
