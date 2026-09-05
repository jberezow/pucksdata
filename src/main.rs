//! CLI entry point — parses [`clap`] commands and dispatches to library functions.
use clap::{Args, Parser, Subcommand};
use pucksdata::{db, fetchers, loaders};

#[derive(Parser)]
#[command(name = "pucksdata", about = "NHL Data ETL Engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch and upsert NHL entity metadata
    Fetch {
        #[command(subcommand)]
        entity: FetchEntity,
    },
    /// Run full historical backfill (events only; entity tables must be pre-populated)
    Backfill(BackfillArgs),
    /// Sync all completed games (game_state OFF/OVER/FINAL) that have no events in the database
    Sync(SyncArgs),
    /// Run as a long-lived daemon, calling sync on a configurable interval
    Daemon(DaemonArgs),
    /// Check DB health per season: game counts, event coverage %, goals-in-shots, backfill status
    Status(StatusArgs),
}

#[derive(Subcommand)]
enum FetchEntity {
    /// Fetch all NHL teams
    Teams,
    /// Fetch all NHL players
    Players,
    /// Fetch all NHL seasons
    Seasons,
    /// Fetch NHL games
    Games(GamesArgs),
    /// Fetch play-by-play events for a game
    Events(EventsArgs),
    /// Fetch official NHL season totals for skaters and goalies
    OfficialStats(OfficialStatsArgs),
}

#[derive(Args)]
struct OfficialStatsArgs {
    /// Restrict the load to a single season (e.g. 20242025)
    #[arg(long)]
    season: Option<i32>,
}

#[derive(Args)]
struct EventsArgs {
    /// Game ID to fetch play-by-play events for
    game_id: i64,
}

#[derive(Args)]
struct GamesArgs {
    #[command(flatten)]
    scope: GamesScope,
}

#[derive(Args)]
struct BackfillArgs {
    /// Restrict backfill to a single season (e.g. 20232024)
    #[arg(long)]
    season: Option<i32>,

    /// Re-fetch and atomically replace games already marked done
    #[arg(long, requires = "season")]
    refresh: bool,
}

#[derive(Args)]
struct SyncArgs {
    /// Override gap detection: re-process all completed games on or after this date (YYYY-MM-DD).
    /// Without this flag, the sync watermark is derived structurally from the database.
    #[arg(long, value_name = "DATE")]
    from: Option<String>,
}

#[derive(Args)]
struct DaemonArgs {
    /// Sync interval in seconds (default: 21600 = 6 hours).
    /// Also read from SYNC_INTERVAL_SECS env var if flag absent.
    #[arg(long, value_name = "SECS")]
    interval_secs: Option<u64>,

    /// Run a full backfill before entering the sync loop.
    #[arg(long)]
    backfill_on_start: bool,
}

#[derive(Args)]
struct StatusArgs {
    /// Restrict status output to a single season (e.g. 20252026)
    #[arg(long)]
    season: Option<i32>,

    /// Fetch game metadata and run backfill to remediate coverage gaps
    #[arg(long)]
    fix: bool,

    /// Emit the health report as JSON
    #[arg(long, conflicts_with = "fix")]
    json: bool,

    /// Return success after producing an unhealthy report
    #[arg(long, requires = "json")]
    no_fail: bool,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct GamesScope {
    /// Fetch a single game by ID
    #[arg(long)]
    game: Option<i64>,
    /// Fetch all games for a season (e.g. 20232024)
    #[arg(long)]
    season: Option<i32>,
    /// Fetch all games across all seasons
    #[arg(long)]
    all: bool,
}

#[tokio::main]
async fn main() -> Result<(), pucksdata::AnyError> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Commands::Fetch { entity } => match entity {
            FetchEntity::Teams => {
                let pool = db::get_pool().await?;
                let records = fetchers::teams::fetch_teams().await?;
                let count = records.len();
                let pb = pucksdata::ui::make_progress_bar(count as u64, "teams");
                loaders::teams::upsert_teams(pool, &records, &pb)
                    .await
                    .inspect_err(|_| pb.finish_and_clear())?;
                pb.finish_and_clear();
            }
            FetchEntity::OfficialStats(args) => {
                let pool = db::get_pool().await?;
                pucksdata::process::official_stats::run_official_stats(pool, args.season).await?;
            }
            FetchEntity::Seasons => {
                let pool = db::get_pool().await?;
                let records = fetchers::seasons::fetch_seasons().await?;
                let count = records.len();
                let pb = pucksdata::ui::make_progress_bar(count as u64, "seasons");
                loaders::seasons::upsert_seasons(pool, &records, &pb)
                    .await
                    .inspect_err(|_| pb.finish_and_clear())?;
                pb.finish_and_clear();
            }
            FetchEntity::Players => {
                use std::time::Duration;
                let pool = db::get_pool().await?;

                let records = fetchers::players::fetch_players(pool).await?;
                let count = records.len();

                // The bulk upsert has no meaningful per-record progress.
                let spinner = {
                    use indicatif::{ProgressBar, ProgressStyle};
                    let s = ProgressBar::new_spinner();
                    s.set_style(
                        ProgressStyle::with_template("{spinner} {msg}")
                            .unwrap()
                            .tick_strings(&[
                                "\u{29fe}", "\u{29fd}", "\u{29fb}", "\u{23bf}", "\u{23bf}",
                                "\u{29df}", "\u{29af}", "\u{29b7}", "",
                            ]),
                    );
                    s.enable_steady_tick(Duration::from_millis(80));
                    s.set_message(format!("Writing {count} players to DB..."));
                    s
                };
                loaders::players::upsert_players(pool, &records)
                    .await
                    .inspect_err(|_| spinner.finish_and_clear())?;
                spinner.finish_and_clear();
                println!("Wrote {count} players");
            }
            FetchEntity::Events(args) => {
                use indicatif::{ProgressBar, ProgressStyle};
                let pool = db::get_pool().await?;
                let pb = ProgressBar::new(3);
                pb.set_style(
                    ProgressStyle::with_template(
                        "[{elapsed_precise}] [{bar:20.cyan/blue}] {pos}/{len} {msg}",
                    )
                    .unwrap()
                    .progress_chars("=>-"),
                );

                pb.set_message("fetching team ID map...");
                let team_id_map = fetchers::games::fetch_team_id_to_franchise_id_map().await?;
                pb.inc(1);

                pb.set_message(format!(
                    "fetching play-by-play for game {}...",
                    args.game_id
                ));
                let pbp = fetchers::events::fetch_play_by_play(args.game_id).await?;
                pb.inc(1);

                pb.set_message("transforming and loading events...");
                let goal_strengths = if fetchers::events::needs_goal_strengths(&pbp) {
                    fetchers::events::fetch_goal_strengths(args.game_id).await?
                } else {
                    std::collections::HashMap::new()
                };
                let report_strengths =
                    fetchers::historical_reports::fetch_reconciled_strengths(&pbp)
                        .await?
                        .strengths;
                let (events, goals, shots, hits, blocks, penalties, faceoffs, skip_warnings) =
                    fetchers::events::transform_events_with_strength_sources(
                        &pbp,
                        &team_id_map,
                        &goal_strengths,
                        &report_strengths,
                    );
                for warning in &skip_warnings {
                    pb.suspend(|| eprintln!("{warning}"));
                }
                let (ec, gc, sc, hc, bc, pc, fc) = loaders::events::upsert_game_events(
                    pool,
                    args.game_id,
                    &events,
                    &goals,
                    &shots,
                    &hits,
                    &blocks,
                    &penalties,
                    &faceoffs,
                )
                .await?;
                pb.inc(1);
                pb.finish_with_message(format!(
                    "game {}: {} events, {} goals, {} shots, {} hits, {} blocks, {} penalties, {} faceoffs",
                    args.game_id, ec, gc, sc, hc, bc, pc, fc
                ));
            }
            FetchEntity::Games(args) => {
                let pool = db::get_pool().await?;

                if let Some(game_id) = args.scope.game {
                    let game = fetchers::games::fetch_single_game(game_id).await?;
                    loaders::games::upsert_games(pool, &[game], &indicatif::ProgressBar::hidden())
                        .await?;
                    println!("Fetched 1 record, upserted 1");
                } else if let Some(season) = args.scope.season {
                    let pb_fetch = pucksdata::ui::make_progress_bar(0, "games fetched");
                    let games =
                        fetchers::games::fetch_games_for_season_enriched(season, &pb_fetch).await;
                    let count = games.len();
                    pb_fetch.finish_and_clear();

                    let pb_upsert = pucksdata::ui::make_progress_bar(count as u64, "games written");
                    loaders::games::upsert_games(pool, &games, &pb_upsert)
                        .await
                        .inspect_err(|_| pb_upsert.finish_and_clear())?;
                    pb_upsert.finish_and_clear();
                } else {
                    let seasons = fetchers::games::fetch_seasons_list().await?;
                    let total_seasons = seasons.len();
                    let mut total_games = 0usize;

                    for (i, season) in seasons.iter().enumerate() {
                        println!(
                            "[{}/{}] Fetching season {}...",
                            i + 1,
                            total_seasons,
                            season
                        );

                        let pb_fetch = pucksdata::ui::make_progress_bar(0, "games fetched");
                        let games =
                            fetchers::games::fetch_games_for_season_enriched(*season, &pb_fetch)
                                .await;
                        let count = games.len();
                        pb_fetch.finish_and_clear();

                        if count > 0 {
                            let pb_upsert =
                                pucksdata::ui::make_progress_bar(count as u64, "games written");
                            loaders::games::upsert_games(pool, &games, &pb_upsert)
                                .await
                                .inspect_err(|_| pb_upsert.finish_and_clear())?;
                            pb_upsert.finish_and_clear();
                        }
                        total_games += count;
                    }
                    println!("Fetched {total_games} total games across {total_seasons} seasons, upserted {total_games}");
                }
            }
        },
        Commands::Backfill(args) => {
            let pool = db::get_pool().await?;
            pucksdata::process::backfill::run_backfill_with_refresh(
                pool,
                args.season,
                args.refresh,
            )
            .await?;
        }
        Commands::Sync(args) => {
            let pool = db::get_pool().await?;
            let from_date = args
                .from
                .as_deref()
                .map(|s| {
                    time::Date::parse(
                        s,
                        &time::macros::format_description!("[year]-[month]-[day]"),
                    )
                    .map_err(|e| format!("invalid --from date '{s}': {e}"))
                })
                .transpose()?;
            pucksdata::process::sync::run_sync(pool, from_date).await?;
        }
        Commands::Daemon(args) => {
            let pool = db::get_pool().await?;
            let interval_secs = args
                .interval_secs
                .or_else(|| {
                    std::env::var("SYNC_INTERVAL_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                })
                .unwrap_or(21600); // 6 hours default
            pucksdata::process::daemon::run_daemon(pool, interval_secs, args.backfill_on_start)
                .await?;
        }
        Commands::Status(args) => {
            let pool = db::get_pool().await?;
            let healthy = if args.json {
                pucksdata::process::status::run_status_json(pool, args.season).await?
            } else {
                pucksdata::process::status::run_status(pool, args.season, args.fix).await?
            };
            if !healthy && !args.no_fail {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
