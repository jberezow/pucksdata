//! Fetches franchise records and abbreviations from the NHL stats API.
use crate::{api::fetch_api_json, models::DbTeam, AnyError};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;

#[derive(serde::Deserialize)]
struct FranchiseRecord {
    id: i64,
    #[serde(rename = "fullName")]
    full_name: String,
    #[serde(rename = "teamCommonName")]
    common_name: String,
    #[serde(rename = "teamPlaceName")]
    place_name: String,
}

#[derive(serde::Deserialize)]
struct TeamAbbrevRecord {
    #[serde(rename = "fullName")]
    full_name: String,
    #[serde(rename = "triCode")]
    tri_code: String,
}

#[derive(serde::Deserialize)]
struct ApiResponse<T> {
    data: Vec<T>,
}

/// Fetch all NHL franchise records from the stats API.
pub async fn fetch_teams() -> Result<Vec<DbTeam>, AnyError> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message("Fetching NHL teams...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let franchise_json =
        fetch_api_json("https://api.nhle.com/stats/rest/en/franchise?limit=-1").await?;
    let franchise_resp: ApiResponse<FranchiseRecord> = serde_json::from_str(&franchise_json)?;

    pb.set_message("Fetching team abbreviations...");

    let abbrev_json = fetch_api_json("https://api.nhle.com/stats/rest/en/team?limit=-1").await?;
    let abbrev_resp: ApiResponse<TeamAbbrevRecord> = serde_json::from_str(&abbrev_json)?;

    // The team endpoint contains one row per franchise era. Matching the current
    // franchise name selects its current abbreviation without relying on ID order.
    let abbrev_map: HashMap<String, String> = abbrev_resp
        .data
        .into_iter()
        .map(|r| (r.full_name, r.tri_code))
        .collect();

    let mut teams = Vec::new();
    for franchise in franchise_resp.data {
        match abbrev_map.get(&franchise.full_name) {
            Some(abbrev) => {
                teams.push(DbTeam {
                    team_id: franchise.id,
                    full_name: franchise.full_name,
                    common_name: franchise.common_name,
                    place_name: franchise.place_name,
                    abbrev: abbrev.clone(),
                });
            }
            None => {
                eprintln!(
                    "Warning: franchise id={} '{}' has no triCode match — skipping",
                    franchise.id, franchise.full_name
                );
            }
        }
    }

    let count = teams.len();
    pb.finish_with_message(format!("Fetched {count} teams"));

    Ok(teams)
}
