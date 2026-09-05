//! Fetches game metadata (stats + boxscore) and builds the team-ID-to-franchise-ID map.
use serde::Deserialize;
use std::collections::HashMap;

use crate::{
    api::{fetch_api_json, ApiError},
    models::DbGame,
    AnyError,
};

/// Generic paginated response wrapper for the NHL stats REST API.
#[derive(Deserialize)]
pub struct StatsApiResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
}

/// Stats /en/game endpoint record.
///
/// The stats API names the visitor fields `visitingTeamId` and
/// `visitingScore`, unlike the boxscore API's nested `awayTeam` object.
#[derive(Deserialize)]
pub struct StatsGameRecord {
    pub id: i64,
    pub season: i32,
    #[serde(rename = "gameDate")]
    pub game_date: String, // "YYYY-MM-DD"
    #[serde(rename = "gameType")]
    pub game_type: i16,
    #[serde(rename = "homeTeamId")]
    pub home_team_id: i64,
    #[serde(rename = "visitingTeamId")]
    pub away_team_id: i64,
    #[serde(rename = "homeScore")]
    pub home_score: Option<i16>,
    #[serde(rename = "visitingScore")]
    pub away_score: Option<i16>,
}

/// Localized name object used by the web API (e.g. venue, venueLocation).
#[derive(Deserialize)]
pub struct LocalizedName {
    pub default: String,
}

/// Team sub-object within a boxscore response.
#[derive(Deserialize)]
pub struct BoxscoreTeam {
    pub id: i64,
    pub score: Option<i16>,
}

/// Boxscore /v1/gamecenter/{id}/boxscore endpoint.
///
/// The web API uses nested `awayTeam` and `homeTeam` objects.
#[derive(Deserialize)]
pub struct BoxscoreGame {
    pub id: i64,
    #[serde(rename = "startTimeUTC")]
    pub start_time_utc: Option<String>, // ISO 8601 UTC string
    #[serde(rename = "gameState")]
    pub game_state: Option<String>,
    pub venue: Option<LocalizedName>,
    #[serde(rename = "venueLocation")]
    pub venue_location: Option<LocalizedName>,
    #[serde(rename = "homeTeam")]
    pub home_team: BoxscoreTeam,
    #[serde(rename = "awayTeam")]
    pub away_team: BoxscoreTeam,
}

/// Deserialization record for the /team endpoint used to build the ID map.
#[derive(Deserialize)]
struct TeamIdRecord {
    id: i64,
    #[serde(rename = "franchiseId")]
    franchise_id: Option<i64>,
}

/// Fetch a map of NHL team ID → franchise ID.
///
/// The stats /game endpoint returns `homeTeamId`/`visitingTeamId` in the NHL team ID space
/// (e.g. VGK=54, SEA=55), but the `teams` table is keyed by franchise ID (e.g. VGK=38, SEA=39).
/// This map is used to translate game team IDs to franchise IDs before DB insertion.
pub async fn fetch_team_id_to_franchise_id_map() -> Result<HashMap<i64, i64>, AnyError> {
    #[derive(Deserialize)]
    struct TeamListResponse {
        data: Vec<TeamIdRecord>,
    }
    let json = fetch_api_json("https://api.nhle.com/stats/rest/en/team?limit=-1").await?;
    let resp: TeamListResponse = serde_json::from_str(&json)?;
    let map = resp
        .data
        .into_iter()
        .filter_map(|r| r.franchise_id.map(|fid| (r.id, fid)))
        .collect();
    Ok(map)
}

/// Fetch all games for a season from the stats endpoint (paginates at limit=500).
///
/// Query field names are `season` and `id`; the otherwise common
/// `seasonId` and `gameId` forms return HTTP 400 here.
pub async fn fetch_games_for_season(season_year: i32) -> Result<Vec<StatsGameRecord>, AnyError> {
    let mut all_games: Vec<StatsGameRecord> = Vec::new();
    let mut start: usize = 0;
    let limit: usize = 500;
    loop {
        let url = format!(
            "https://api.nhle.com/stats/rest/en/game?limit={limit}&start={start}&sort=id&dir=asc&cayenneExp=season%3D{season_year}"
        );
        let json = fetch_api_json(&url).await?;
        let resp: StatsApiResponse<StatsGameRecord> = serde_json::from_str(&json)?;
        let batch_len = resp.data.len();
        all_games.extend(resp.data);
        if batch_len == 0 || all_games.len() >= resp.total as usize {
            break;
        }
        start += limit;
    }
    Ok(all_games)
}

/// Fetch the boxscore for a single game. On serde parse failure returns ApiError::Other(0).
pub async fn fetch_game_boxscore(game_id: i64) -> Result<BoxscoreGame, ApiError> {
    let url = format!("https://api-web.nhle.com/v1/gamecenter/{game_id}/boxscore");
    let json = fetch_api_json(&url).await?;
    serde_json::from_str(&json).map_err(|_e| ApiError::Other(0))
}

/// Fetch the list of all season year integers from the seasons endpoint (for --all mode).
pub async fn fetch_seasons_list() -> Result<Vec<i32>, AnyError> {
    let json = fetch_api_json("https://api-web.nhle.com/v1/season").await?;
    let years: Vec<i32> = serde_json::from_str(&json)?;
    Ok(years)
}

/// Merge a stats record and an optional boxscore into a DbGame.
///
/// `team_id_map` translates NHL team IDs (as returned by the stats /game endpoint's
/// `homeTeamId`/`visitingTeamId` fields) to franchise IDs, which are the primary keys
/// stored in the `teams` table. Without this translation the FK constraint on
/// `games(home_team_id)` / `games(away_team_id)` will fire for any team whose NHL team ID
/// differs from its franchise ID (e.g. VGK: team_id=54, franchise_id=38).
pub fn transform_game(
    stats: &StatsGameRecord,
    boxscore: Option<&BoxscoreGame>,
    team_id_map: &HashMap<i64, i64>,
) -> Result<DbGame, AnyError> {
    use time::macros::format_description;
    let game_date = time::Date::parse(
        &stats.game_date,
        format_description!("[year]-[month]-[day]"),
    )?;

    let (start_time_utc, venue, venue_location, game_state) = match boxscore {
        Some(bs) => {
            let ts = bs
                .start_time_utc
                .as_deref()
                .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
            let v = bs.venue.as_ref().map(|v| v.default.clone());
            let vl = bs.venue_location.as_ref().map(|v| v.default.clone());
            let gs = bs.game_state.clone();
            (ts, v, vl, gs)
        }
        None => (None, None, None, None),
    };

    // Translate NHL team IDs → franchise IDs (teams table primary key).
    //
    // Fallback to raw ID is intentionally NOT used here: non-NHL team IDs (e.g. European
    // exhibition clubs like EHC Red Bull München / id=7509) are absent from both the /team
    // endpoint and the teams table. Inserting the raw ID would always produce an FK violation.
    // Instead we surface an Err so the caller can skip + warn for that game.
    let home_team_id = team_id_map
        .get(&stats.home_team_id)
        .copied()
        .ok_or_else(|| {
            format!(
                "unmapped home_team_id {} for game {}",
                stats.home_team_id, stats.id
            )
        })?;
    let away_team_id = team_id_map
        .get(&stats.away_team_id)
        .copied()
        .ok_or_else(|| {
            format!(
                "unmapped away_team_id {} for game {}",
                stats.away_team_id, stats.id
            )
        })?;

    Ok(DbGame {
        game_id: stats.id,
        season: stats.season,
        game_date,
        start_time_utc,
        home_team_id,
        away_team_id,
        game_type: stats.game_type,
        venue,
        venue_location,
        game_state,
        home_score: stats.home_score,
        away_score: stats.away_score,
    })
}

// ── Bulk enriched fetch ───────────────────────────────────────────────────────

const MAX_CONCURRENT_BOXSCORES: usize = 10;

/// Game types represented by NHL-franchise teams in this database.
fn is_supported_game_type(game_type: i16) -> bool {
    matches!(game_type, 1..=4)
}

/// Enumerate all games for a season, concurrently fetch their boxscores (10-permit semaphore),
/// transform and return DbGame records. Individual game errors skip + warn.
pub async fn fetch_games_for_season_enriched(
    season_year: i32,
    pb: &indicatif::ProgressBar,
) -> Vec<DbGame> {
    let team_id_map = match fetch_team_id_to_franchise_id_map().await {
        Ok(m) => m,
        Err(e) => {
            pb.suspend(|| eprintln!("warn: failed to fetch team ID map: {e}"));
            return Vec::new();
        }
    };

    let mut stats_records = match fetch_games_for_season(season_year).await {
        Ok(r) => r,
        Err(e) => {
            pb.suspend(|| eprintln!("warn: failed to fetch games for season {season_year}: {e}"));
            return Vec::new();
        }
    };

    let fetched_count = stats_records.len();
    stats_records.retain(|game| is_supported_game_type(game.game_type));
    let unsupported_count = fetched_count - stats_records.len();
    if unsupported_count > 0 {
        pb.suspend(|| {
            eprintln!(
                "info: skipped {unsupported_count} out-of-scope game(s) for season {season_year}"
            )
        });
    }

    pb.set_length(stats_records.len() as u64);
    pb.set_message(format!("games (season {season_year:08})"));

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_BOXSCORES));
    let team_id_map = std::sync::Arc::new(team_id_map);
    let mut join_set: tokio::task::JoinSet<(StatsGameRecord, Option<BoxscoreGame>)> =
        tokio::task::JoinSet::new();

    for stats in stats_records {
        let sem = semaphore.clone();
        join_set.spawn(async move {
            let _permit = sem.acquire_owned().await.unwrap();
            let bs = match fetch_game_boxscore(stats.id).await {
                Ok(b) => Some(b),
                // 404 is expected for unplayed games (e.g. playoff series that ended
                // before game 6 or 7). Silently use stats-only data — no warning needed.
                Err(crate::api::ApiError::NotFound) => None,
                Err(e) => {
                    eprintln!(
                        "warn: boxscore fetch failed for game {}: {e:?}, using stats-only data",
                        stats.id
                    );
                    None
                }
            };
            (stats, bs)
        });
    }

    let mut games = Vec::new();
    while let Some(res) = join_set.join_next().await {
        pb.inc(1);
        match res {
            Ok((stats, bs)) => match transform_game(&stats, bs.as_ref(), &team_id_map) {
                Ok(game) => games.push(game),
                Err(e) => {
                    pb.suspend(|| eprintln!("warn: transform failed for game {}: {e}", stats.id))
                }
            },
            Err(e) => pb.suspend(|| eprintln!("warn: task join error: {e}")),
        }
    }
    games
}

// ── Single game fetch ─────────────────────────────────────────────────────────

/// Fetch a single game by ID (for --game mode). Fetches stats + boxscore and transforms.
pub async fn fetch_single_game(game_id: i64) -> Result<DbGame, AnyError> {
    let team_id_map = fetch_team_id_to_franchise_id_map().await?;
    let url = format!("https://api.nhle.com/stats/rest/en/game?cayenneExp=id%3D{game_id}");
    let json = fetch_api_json(&url).await?;
    let resp: StatsApiResponse<StatsGameRecord> = serde_json::from_str(&json)?;
    let stats = resp
        .data
        .into_iter()
        .next()
        .ok_or_else(|| format!("game {game_id} not found in stats API"))?;
    let bs = fetch_game_boxscore(game_id).await.ok();
    transform_game(&stats, bs.as_ref(), &team_id_map)
}

#[cfg(test)]
mod tests {
    use super::is_supported_game_type;

    #[test]
    fn supported_game_types_exclude_international_games() {
        for game_type in 1..=4 {
            assert!(is_supported_game_type(game_type));
        }
        assert!(!is_supported_game_type(9));
        assert!(!is_supported_game_type(18));
    }
}
