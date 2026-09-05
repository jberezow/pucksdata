//! Historical backfill orchestrator — seeds, checkpoints, and processes all pending games.

/// Seed backfill_progress for all games in scope.
/// INSERT ... ON CONFLICT DO NOTHING so existing rows (done/failed) survive unchanged.
/// season_filter: None = all seasons, Some(year) = restrict to one season.
pub async fn seed_backfill_progress(
    pool: &sqlx::PgPool,
    season_filter: Option<i32>,
) -> Result<(), sqlx::Error> {
    seed_backfill_progress_with_refresh(pool, season_filter, false).await
}

/// Seed a season and optionally reset its completed checkpoints for an
/// authoritative replay.
pub async fn seed_backfill_progress_with_refresh(
    pool: &sqlx::PgPool,
    season_filter: Option<i32>,
    refresh: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO backfill_progress (game_id, season, status)
         SELECT game_id, season, 'pending'
         FROM games
         WHERE ($1::integer IS NULL OR season = $1::integer)
           AND game_state NOT IN ('FUT', 'PRE')
         ON CONFLICT (game_id) DO UPDATE
         SET status = CASE WHEN $2::boolean THEN 'pending' ELSE backfill_progress.status END,
             error_message = CASE WHEN $2::boolean THEN NULL ELSE backfill_progress.error_message END,
             updated_at = CASE WHEN $2::boolean THEN NOW() ELSE backfill_progress.updated_at END"
    )
    .bind(season_filter)
    .bind(refresh)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update a game's status in backfill_progress.
/// Call with "done" on success, "failed" on error.
pub async fn update_progress_status(
    pool: &sqlx::PgPool,
    game_id: i64,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE backfill_progress
         SET status = $1, updated_at = NOW()
         WHERE game_id = $2",
        status,
        game_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Update game status and store error message.
/// Use for 'failed' transitions. Keep update_progress_status for 'done'/'skipped'.
pub async fn update_progress_with_error(
    pool: &sqlx::PgPool,
    game_id: i64,
    status: &str,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE backfill_progress
         SET status = $1, error_message = $2, updated_at = NOW()
         WHERE game_id = $3",
        status,
        error_message,
        game_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns true if the error represents a known API gap (404 Not Found).
/// These games are classified as 'skipped', not 'failed' — they will not be retried.
/// Uses downcast_ref — type-safe, not string matching on error messages.
pub fn is_api_gap_error(e: &crate::AnyError) -> bool {
    e.downcast_ref::<crate::api::ApiError>()
        .map(|api_err| matches!(api_err, crate::api::ApiError::NotFound))
        .unwrap_or(false)
}

/// Per-game metadata returned by query_pending_games.
/// Carries game_date, home_abbrev, and away_abbrev for log line emission.
pub struct PendingGame {
    pub game_id: i64,
    pub season: i32,
    pub game_date: time::Date,
    pub home_abbrev: String,
    pub away_abbrev: String,
}

/// Query all non-done games in scope (returns `Vec<PendingGame>` with joined metadata).
/// Used after seeding to build the work list for the current run.
/// Includes both 'pending' and 'failed' games (failed games are retried).
pub async fn query_pending_games(
    pool: &sqlx::PgPool,
    season_filter: Option<i32>,
) -> Result<Vec<PendingGame>, sqlx::Error> {
    let rows = sqlx::query_as!(
        PendingGame,
        "SELECT bp.game_id, bp.season,
                g.game_date,
                ht.abbrev AS home_abbrev,
                at_.abbrev AS away_abbrev
         FROM backfill_progress bp
         JOIN games g ON g.game_id = bp.game_id
         JOIN teams ht ON ht.team_id = g.home_team_id
         JOIN teams at_ ON at_.team_id = g.away_team_id
         WHERE ($1::integer IS NULL OR bp.season = $1)
           AND bp.status NOT IN ('done', 'skipped')
           AND g.game_state NOT IN ('FUT', 'PRE')
         ORDER BY bp.season ASC, bp.game_id ASC",
        season_filter
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Fetch, transform, and load all events for one game.
/// Called inside a JoinSet task — errors are captured, not propagated.
/// Returns the total event count (sum of all event type counts) on success.
pub async fn load_one_game(
    pool: &sqlx::PgPool,
    game_id: i64,
    team_id_map: &std::collections::HashMap<i64, i64>,
) -> Result<usize, crate::AnyError> {
    let pbp = crate::fetchers::events::fetch_play_by_play(game_id).await?;
    let goal_strengths = if crate::fetchers::events::needs_goal_strengths(&pbp) {
        crate::fetchers::events::fetch_goal_strengths(game_id).await?
    } else {
        std::collections::HashMap::new()
    };
    let report_strengths = crate::fetchers::historical_reports::fetch_reconciled_strengths(&pbp)
        .await?
        .strengths;
    let (events, goals, shots, hits, blocks, penalties, faceoffs, skip_warnings) =
        crate::fetchers::events::transform_events_with_strength_sources(
            &pbp,
            team_id_map,
            &goal_strengths,
            &report_strengths,
        );
    // skip_warnings are swallowed in batch mode (volume too high for per-game warnings)
    let _ = skip_warnings;
    let (ec, gc, sc, hc, bc, pc, fc) = crate::loaders::events::upsert_game_events(
        pool, game_id, &events, &goals, &shots, &hits, &blocks, &penalties, &faceoffs,
    )
    .await?;
    Ok(ec + gc + sc + hc + bc + pc + fc)
}

/// Result type for a single backfill game task spawned in the JoinSet.
/// Carries (game_id, season, game_date, home_abbrev, away_abbrev, fetch_result).
type BackfillTaskResult = (
    i64,
    i32,
    time::Date,
    String,
    String,
    Result<usize, crate::AnyError>,
);

/// Run the full (or season-scoped) backfill.
/// season_filter: None = all seasons, Some(year) = one 8-digit season ID (e.g. 20232024)
pub async fn run_backfill(
    pool: &sqlx::PgPool,
    season_filter: Option<i32>,
) -> Result<(), crate::AnyError> {
    run_backfill_with_refresh(pool, season_filter, false).await
}

/// Run a backfill and optionally reprocess already-completed games in one
/// explicitly selected season.
pub async fn run_backfill_with_refresh(
    pool: &sqlx::PgPool,
    season_filter: Option<i32>,
    refresh: bool,
) -> Result<(), crate::AnyError> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::task::JoinSet;

    const MAX_CONCURRENT_GAMES: usize = 5;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(&[
                "\u{29fe}", "\u{29fd}", "\u{29fb}", "\u{23bf}", "\u{23bf}", "\u{29df}", "\u{29af}",
                "\u{29b7}", "",
            ]),
    );
    spinner.enable_steady_tick(Duration::from_millis(80));

    spinner.set_message("Fetching team ID map...");
    let team_id_map = Arc::new(
        crate::fetchers::games::fetch_team_id_to_franchise_id_map()
            .await
            .inspect_err(|_| spinner.finish_and_clear())?,
    );

    spinner.set_message("Seeding backfill queue...");
    seed_backfill_progress_with_refresh(pool, season_filter, refresh)
        .await
        .inspect_err(|_| spinner.finish_and_clear())?;

    spinner.set_message("Loading pending games...");
    let pending_games = query_pending_games(pool, season_filter)
        .await
        .inspect_err(|_| spinner.finish_and_clear())?;
    let total = pending_games.len();
    spinner.finish_and_clear();

    if total == 0 {
        println!("Backfill complete: 0 games pending (all already done)");
        return Ok(());
    }

    let pb = crate::ui::make_progress_bar(total as u64, "games");

    // Keep the task window full without allocating one future per historical game.
    let mut join_set: JoinSet<BackfillTaskResult> = JoinSet::new();

    let mut season_done: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    let mut season_failed: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    let mut season_skipped: std::collections::HashMap<i32, usize> =
        std::collections::HashMap::new();

    let mut total_done = 0usize;
    let mut total_failed = 0usize;
    let mut total_skipped = 0usize;

    let backfill_start = std::time::Instant::now();

    let mut games_iter = pending_games.into_iter();

    macro_rules! spawn_game {
        ($game:expr) => {{
            let game = $game;
            let game_id = game.game_id;
            let season = game.season;
            let game_date = game.game_date;
            let home_abbrev = game.home_abbrev.clone();
            let away_abbrev = game.away_abbrev.clone();
            let pool_clone = pool.clone();
            let map = team_id_map.clone();
            join_set.spawn(async move {
                let result = load_one_game(&pool_clone, game_id, &map).await;
                (game_id, season, game_date, home_abbrev, away_abbrev, result)
            });
        }};
    }

    for game in (&mut games_iter).take(MAX_CONCURRENT_GAMES) {
        spawn_game!(game);
    }

    while let Some(outcome) = join_set.join_next().await {
        match outcome {
            Ok((game_id, season, game_date, home_abbrev, away_abbrev, Ok(_count))) => {
                pb.suspend(|| println!("{game_date}  {game_id}  {home_abbrev} vs {away_abbrev}"));
                update_progress_status(pool, game_id, "done")
                    .await
                    .unwrap_or_else(|e| {
                        pb.suspend(|| {
                            eprintln!("warn: checkpoint update failed for game {game_id}: {e}")
                        })
                    });
                *season_done.entry(season).or_insert(0) += 1;
                total_done += 1;
            }
            Ok((game_id, season, game_date, home_abbrev, away_abbrev, Err(e))) => {
                if is_api_gap_error(&e) {
                    pb.suspend(|| {
                        println!(
                            "{game_date}  {game_id}  {home_abbrev} vs {away_abbrev}  [SKIPPED]"
                        )
                    });
                    update_progress_with_error(pool, game_id, "skipped", &e.to_string())
                        .await
                        .unwrap_or_else(|e2| {
                            pb.suspend(|| {
                                eprintln!("warn: checkpoint update failed for game {game_id}: {e2}")
                            })
                        });
                    *season_skipped.entry(season).or_insert(0) += 1;
                    total_skipped += 1;
                } else {
                    pb.suspend(|| {
                        println!("{game_date}  {game_id}  {home_abbrev} vs {away_abbrev}  [FAILED]")
                    });
                    pb.suspend(|| eprintln!("warn: game {game_id} (season {season}) failed: {e}"));
                    update_progress_with_error(pool, game_id, "failed", &e.to_string())
                        .await
                        .unwrap_or_else(|e2| {
                            pb.suspend(|| {
                                eprintln!("warn: checkpoint update failed for game {game_id}: {e2}")
                            })
                        });
                    *season_failed.entry(season).or_insert(0) += 1;
                    total_failed += 1;
                }
            }
            Err(join_err) => {
                pb.suspend(|| eprintln!("warn: task join error: {join_err}"));
                total_failed += 1;
            }
        }
        pb.inc(1);
        // Spawn the next game now that a slot is free
        if let Some(game) = games_iter.next() {
            spawn_game!(game);
        }
    }

    pb.finish_and_clear();

    // Per-season summaries
    let mut all_seasons: std::collections::BTreeSet<i32> = std::collections::BTreeSet::new();
    all_seasons.extend(season_done.keys());
    all_seasons.extend(season_failed.keys());
    all_seasons.extend(season_skipped.keys());
    for season in &all_seasons {
        let done = season_done.get(season).copied().unwrap_or(0);
        let failed = season_failed.get(season).copied().unwrap_or(0);
        let skipped = season_skipped.get(season).copied().unwrap_or(0);
        println!("Season {season}: {done} done, {failed} failed, {skipped} skipped");
    }

    // Final summary
    let elapsed = backfill_start.elapsed();
    let total_processed = total_done + total_failed + total_skipped;
    println!(
        "Backfill complete:\n  processed: {}\n  succeeded: {}\n  failed:    {}\n  skipped:   {}\n  duration:  {:.1}s",
        total_processed,
        total_done,
        total_failed,
        total_skipped,
        elapsed.as_secs_f64()
    );

    // Gap repair: fetch any player IDs present in event tables but absent from players.
    // Catches retired/AHL players that slipped through enumerate_player_ids, including
    // historical players like Sergei Kostitsyn (20072008-20122013) already written to goals.
    match crate::fetchers::players::repair_missing_players(pool).await {
        Ok(0) => {}
        Ok(n) => println!("repair: inserted {n} previously-missing players"),
        Err(e) => eprintln!("warn: repair_missing_players failed (non-fatal): {e}"),
    }

    // Any game touched moves the health snapshot, including one that only
    // recorded a failure.
    if total_processed > 0 {
        crate::process::analytics::refresh_derived(pool).await;
    }

    Ok(())
}
