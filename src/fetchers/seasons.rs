//! Fetches the list of all season IDs from the NHL stats API.
use crate::{api::fetch_api_json, models::DbSeason, AnyError};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;

#[derive(serde::Deserialize)]
struct StatsSeasonRecord {
    #[serde(rename = "seasonId", alias = "id")]
    season_id: i32,
    #[serde(rename = "startDate")]
    start_date: Option<String>,
    #[serde(rename = "endDate")]
    end_date: Option<String>,
    #[serde(rename = "regularSeasonEndDate")]
    regular_season_end_date: Option<String>,
}

#[derive(serde::Deserialize)]
struct StatsSeasonResponse {
    data: Vec<StatsSeasonRecord>,
}

fn parse_date(s: &str) -> Option<time::Date> {
    use time::macros::format_description;
    let fmt = format_description!("[year]-[month]-[day]");
    s.get(..10)
        .and_then(|date| time::Date::parse(date, fmt).ok())
}

/// Fetch all NHL season IDs from the stats API.
pub async fn fetch_seasons() -> Result<Vec<DbSeason>, AnyError> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message("Fetching NHL seasons...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let seasons_json = fetch_api_json("https://api-web.nhle.com/v1/season").await?;
    let season_years: Vec<i32> = serde_json::from_str(&seasons_json)?;

    pb.set_message("Fetching season date data...");

    let stats_map: HashMap<i32, StatsSeasonRecord> =
        match fetch_api_json("https://api.nhle.com/stats/rest/en/season?limit=-1").await {
            Ok(json) => match serde_json::from_str::<StatsSeasonResponse>(&json) {
                Ok(resp) => resp.data.into_iter().map(|r| (r.season_id, r)).collect(),
                Err(e) => {
                    eprintln!(
                        "Warning: failed to parse stats season data: {e} — dates will be None"
                    );
                    HashMap::new()
                }
            },
            Err(crate::api::ApiError::NotFound) => {
                eprintln!("Warning: stats season endpoint returned 404 — dates will be None");
                HashMap::new()
            }
            Err(e) => {
                eprintln!("Warning: stats season endpoint error: {e} — dates will be None");
                HashMap::new()
            }
        };

    let mut seasons = Vec::new();
    for season_year in season_years {
        let stats = stats_map.get(&season_year);
        let start_date = stats
            .and_then(|s| s.start_date.as_deref())
            .and_then(parse_date);
        let end_date = stats
            .and_then(|s| s.end_date.as_deref())
            .and_then(parse_date);
        let regular_season_end_date = stats
            .and_then(|s| s.regular_season_end_date.as_deref())
            .and_then(parse_date);

        seasons.push(DbSeason {
            season_year,
            start_date,
            end_date,
            regular_season_end_date,
        });
    }

    let count = seasons.len();
    pb.finish_with_message(format!("Fetched {count} seasons"));

    Ok(seasons)
}

#[cfg(test)]
mod tests {
    use super::{parse_date, StatsSeasonResponse};
    use time::macros::date;

    #[test]
    fn parses_current_stats_season_schema() {
        let json = r#"{"data":[{"id":20252026,"startDate":"2025-10-07T17:00:00","endDate":"2026-06-15T00:00:00","regularSeasonEndDate":"2026-04-17T00:00:00"}]}"#;
        let response: StatsSeasonResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.data[0].season_id, 20252026);
        assert_eq!(
            response.data[0].start_date.as_deref().and_then(parse_date),
            Some(date!(2025 - 10 - 07))
        );
    }

    #[test]
    fn retains_legacy_stats_season_compatibility() {
        let json = r#"{"data":[{"seasonId":20242025,"startDate":"2024-10-04","endDate":null,"regularSeasonEndDate":null}]}"#;
        let response: StatsSeasonResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.data[0].season_id, 20242025);
        assert_eq!(
            response.data[0].start_date.as_deref().and_then(parse_date),
            Some(date!(2024 - 10 - 04))
        );
    }
}
