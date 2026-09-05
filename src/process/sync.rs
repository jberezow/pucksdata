//! Incremental sync orchestrator — gap detection and event ingestion for completed games.
use chrono::Datelike;
use sqlx::postgres::PgAdvisoryLock;
use sqlx::Either;

/// Summary returned by run_sync() on every success path.
pub struct SyncSummary {
    pub processed: usize,
    pub failed: usize,
    pub elapsed: std::time::Duration,
    pub candidates: usize,
    pub events_written: usize,
}

/// Returns true if this gameState value indicates the game is definitively finished.
/// Accepted completed states: "OFF", "OVER", "FINAL".
/// Any other value is unknown — caller should log a warning and skip.
pub fn is_game_completed(state: &str) -> bool {
    matches!(state, "OFF" | "OVER" | "FINAL")
}

/// Derive the 8-digit NHL season ID for a given calendar month and year.
///
/// NHL seasons span two calendar years (e.g. 2025-2026 season ID = 20252026).
/// A new season starts in October. Months before October (1–9) belong to the
/// season that started the previous October.
///
/// Examples:
///   season_for_date(10, 2025) == 20252026  (Oct 2025 → start of 2025-26 season)
///   season_for_date(3,  2026) == 20252026  (Mar 2026 → mid 2025-26 season)
///   season_for_date(9,  2025) == 20242025  (Sep 2025 → offseason, 2024-25 was last)
pub fn season_for_date(month: u32, year: i32) -> i32 {
    if month >= 10 {
        // October onwards: new season starting this year
        year * 10_000 + (year + 1)
    } else {
        // January–September: season started last October
        (year - 1) * 10_000 + year
    }
}

/// Return the 8-digit NHL season ID for the current UTC date.
///
/// Calls `season_for_date` with the current UTC month and year.
pub fn current_season() -> i32 {
    let now = chrono::Utc::now();
    season_for_date(now.month(), now.year())
}

async fn refresh_current_season_games(pool: &sqlx::PgPool) -> Result<usize, crate::AnyError> {
    let season = current_season();
    println!("[sync 0/5] refreshing game metadata for season {season}...");
    let progress = indicatif::ProgressBar::hidden();
    let games = crate::fetchers::games::fetch_games_for_season_enriched(season, &progress).await;
    let count = games.len();
    crate::loaders::games::upsert_games(pool, &games, &progress).await?;
    println!("[sync 0/5] {count} games upserted for season {season}");
    Ok(count)
}

/// Acquire the session-level advisory lock used to enforce a single daemon.
///
/// The caller must retain the guard for the daemon's lifetime; dropping it
/// releases the lock and returns its connection to the pool.
pub async fn acquire_daemon_lock(
    pool: &sqlx::PgPool,
) -> Result<
    sqlx::postgres::PgAdvisoryLockGuard<sqlx::pool::PoolConnection<sqlx::Postgres>>,
    crate::AnyError,
> {
    let lock = PgAdvisoryLock::new("pucksdata_daemon");
    let conn = pool.acquire().await?;
    match lock.try_acquire(conn).await? {
        Either::Left(guard) => Ok(guard),
        Either::Right(_conn) => Err(
            "pucksdata daemon is already running (advisory lock held by another instance)".into(),
        ),
    }
}

/// Return past games with no events, optionally bounded by a starting date.
///
/// Game-state filtering remains in Rust so unknown states can be reported.
pub async fn query_sync_candidates(
    pool: &sqlx::PgPool,
    from_date: Option<time::Date>,
) -> Result<Vec<(i64, Option<String>)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT g.game_id, g.game_state
           FROM games g
           WHERE g.game_date < CURRENT_DATE
             AND ($1::date IS NULL OR g.game_date >= $1)
             AND g.game_type != 1
             AND NOT EXISTS (
               SELECT 1 FROM events e WHERE e.game_id = g.game_id
             )
             AND NOT EXISTS (
               SELECT 1 FROM backfill_progress bp
               WHERE bp.game_id = g.game_id
                 AND bp.status IN ('done', 'skipped')
             )
           ORDER BY g.game_date ASC, g.game_id ASC"#,
        from_date
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.game_id, r.game_state))
        .collect())
}

/// Refresh entities and ingest completed games that have no events.
pub async fn run_sync(
    pool: &sqlx::PgPool,
    from_date: Option<time::Date>,
) -> Result<SyncSummary, crate::AnyError> {
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    let started_at = Instant::now();

    refresh_current_season_games(pool).await?;

    // Refresh teams before building the team-to-franchise map.
    println!("[sync 1/5] refreshing teams...");
    let teams = crate::fetchers::teams::fetch_teams().await?;
    crate::loaders::teams::upsert_teams(pool, &teams, &indicatif::ProgressBar::hidden()).await?;
    println!("[sync 1/5] {} teams upserted", teams.len());

    println!("[sync 2/5] enumerating and refreshing players (rosters + stats pages — this takes ~30s)...");
    let players = crate::fetchers::players::fetch_players(pool).await?;
    crate::loaders::players::upsert_players(pool, &players).await?;
    println!("[sync 2/5] {} players upserted", players.len());

    let team_id_map = Arc::new(crate::fetchers::games::fetch_team_id_to_franchise_id_map().await?);

    println!("[sync 3/5] detecting games with missing events...");
    let candidates = query_sync_candidates(pool, from_date).await?;
    let candidates_count = candidates.len(); // all gap-detected candidates, before game_state filter
    println!("[sync 3/5] {candidates_count} candidate games found (regular season + playoffs, no events yet)");

    let mut games_to_process: Vec<i64> = Vec::new();
    for (game_id, state) in &candidates {
        match state.as_deref() {
            Some(s) if is_game_completed(s) => games_to_process.push(*game_id),
            Some(s) => eprintln!("warn: unknown gameState {s:?} for game {game_id} — skipping"),
            None => {} // NULL game_state — not completed, skip silently
        }
    }

    let total = games_to_process.len();
    println!("[sync 4/5] {total} games ready to process (game_state OFF/OVER/FINAL)");

    let (processed, failed, events_written) = if total == 0 {
        println!("[sync 4/5] nothing to do");
        (0usize, 0usize, 0usize)
    } else {
        const MAX_CONCURRENT_GAMES: usize = 5;
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_GAMES));
        let mut join_set: JoinSet<(i64, Result<usize, crate::AnyError>)> = JoinSet::new();

        let pb = crate::ui::make_progress_bar(total as u64, "games");

        for game_id in games_to_process.iter() {
            let permit = sem.clone().acquire_owned().await.expect("semaphore closed");
            let game_id = *game_id;
            let pool_clone = pool.clone();
            let map = team_id_map.clone();

            join_set.spawn(async move {
                let _permit = permit; // released on drop
                let result =
                    crate::process::backfill::load_one_game(&pool_clone, game_id, &map).await;
                (game_id, result)
            });
        }

        let mut processed = 0usize;
        let mut failed = 0usize;
        let mut events_written = 0usize;

        while let Some(outcome) = join_set.join_next().await {
            match outcome {
                Ok((_, Ok(count))) => {
                    events_written += count;
                    processed += 1;
                }
                Ok((game_id, Err(e))) => {
                    pb.suspend(|| eprintln!("warn: game {game_id} failed: {e}"));
                    failed += 1;
                }
                Err(join_err) => {
                    pb.suspend(|| eprintln!("warn: task join error: {join_err}"));
                    failed += 1;
                }
            }
            pb.inc(1);
        }
        pb.finish_and_clear();
        (processed, failed, events_written)
    };

    let elapsed = started_at.elapsed();
    let duration_secs = elapsed.as_secs_f64();
    println!(
        "[sync 5/5] complete:\n  candidates:  {candidates_count}\n  processed:   {processed}\n  failed:      {failed}\n  events:      {events_written}\n  duration:    {duration_secs:.1}s",
    );

    // Repair player references after all new event rows are visible.
    match crate::fetchers::players::repair_missing_players(pool).await {
        Ok(0) => {}
        Ok(n) => println!("repair: inserted {n} previously-missing players"),
        Err(e) => eprintln!("warn: repair_missing_players failed (non-fatal): {e}"),
    }

    // Record successful zero-work syncs as well as runs that ingested games.
    let now = time::OffsetDateTime::now_utc();
    sqlx::query!(
        r#"INSERT INTO sync_state (key, last_sync_at, last_sync_games, updated_at)
       VALUES ('singleton', $1, $2, $1)
       ON CONFLICT (key) DO UPDATE
         SET last_sync_at    = EXCLUDED.last_sync_at,
             last_sync_games = EXCLUDED.last_sync_games,
             updated_at      = EXCLUDED.updated_at"#,
        now,
        processed as i32
    )
    .execute(pool)
    .await?;

    // Only when the picture changed. Most daemon ticks find no candidates, and
    // the refresh is far more expensive than the sync that triggered it. A
    // failed game changes the health snapshot even though it wrote no events.
    if processed > 0 || failed > 0 {
        crate::process::analytics::refresh_derived(pool).await;
    }

    Ok(SyncSummary {
        processed,
        failed,
        elapsed,
        candidates: candidates_count,
        events_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_game_completed() {
        assert!(is_game_completed("OFF"));
        assert!(is_game_completed("OVER"));
        assert!(is_game_completed("FINAL"));
        assert!(!is_game_completed("LIVE"));
        assert!(!is_game_completed("PPD"));
        assert!(!is_game_completed("FUT"));
        assert!(!is_game_completed(""));
    }

    #[test]
    fn test_sync_summary_fields() {
        let s = SyncSummary {
            processed: 3,
            failed: 1,
            elapsed: std::time::Duration::ZERO,
            candidates: 5,
            events_written: 120,
        };
        assert_eq!(s.candidates, 5);
        assert_eq!(s.events_written, 120);
        assert_eq!(s.processed, 3);
        assert_eq!(s.failed, 1);
    }
}
