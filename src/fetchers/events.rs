//! Fetches and transforms play-by-play JSON into typed DB event structs.
use serde::Deserialize;
use std::collections::HashMap;

use crate::{
    api::fetch_api_json,
    models::{DbBlock, DbEvent, DbFaceoff, DbGoal, DbHit, DbPenalty, DbShot, StrengthSource},
    AnyError,
};

// ── Play-by-Play deserialization structs ─────────────────────────────────────

/// Top-level play-by-play response from api-web.nhle.com/v1/gamecenter/{id}/play-by-play
#[derive(Deserialize)]
pub struct PlayByPlay {
    pub id: i64,
    #[serde(rename = "homeTeam")]
    pub home_team: PbpTeam,
    #[serde(rename = "awayTeam")]
    pub away_team: PbpTeam,
    pub plays: Vec<Play>,
}

/// Team sub-object in the play-by-play response.
/// Contains the NHL team ID (NOT franchise ID — must translate via team_id_map).
#[derive(Deserialize)]
pub struct PbpTeam {
    pub id: i64, // NHL team ID — NOT franchise ID
    #[serde(default)]
    pub abbrev: Option<String>,
}

/// A single play/event from the plays array.
#[derive(Deserialize)]
pub struct Play {
    #[serde(rename = "eventId")]
    pub event_id: i32,
    #[serde(rename = "periodDescriptor")]
    pub period_descriptor: PeriodDescriptor,
    #[serde(rename = "timeInPeriod")]
    pub time_in_period: String, // "MM:SS"
    #[serde(rename = "situationCode")]
    pub situation_code: Option<String>, // "1551", "1541", "0651", etc.
    #[serde(rename = "typeDescKey")]
    pub type_desc_key: String, // "goal", "shot-on-goal", "hit", "blocked-shot", "penalty", "faceoff"
    pub details: Option<EventDetails>,
}

/// Period descriptor sub-object.
#[derive(Deserialize)]
pub struct PeriodDescriptor {
    pub number: i16,
    #[serde(rename = "periodType")]
    pub period_type: String, // "REG", "OT", "SO"
}

/// Unified flat struct for all possible event detail fields.
///
/// Optional fields use `serde(default)` because event payloads vary by type.
/// All six event types share this single struct.
#[derive(Deserialize)]
pub struct EventDetails {
    #[serde(rename = "xCoord", default)]
    pub x_coord: Option<i16>,
    #[serde(rename = "yCoord", default)]
    pub y_coord: Option<i16>,
    #[serde(rename = "zoneCode", default)]
    pub zone_code: Option<String>,
    #[serde(rename = "eventOwnerTeamId", default)]
    pub event_owner_team_id: Option<i64>,

    // Goal fields
    #[serde(rename = "scoringPlayerId", default)]
    pub scoring_player_id: Option<i64>,
    #[serde(rename = "assist1PlayerId", default)]
    pub assist1_player_id: Option<i64>,
    #[serde(rename = "assist2PlayerId", default)]
    pub assist2_player_id: Option<i64>,
    #[serde(rename = "goalieInNetId", default)]
    pub goalie_in_net_id: Option<i64>,
    #[serde(rename = "shotType", default)]
    pub shot_type: Option<String>,

    // Shot fields
    #[serde(rename = "shootingPlayerId", default)]
    pub shooting_player_id: Option<i64>,

    // Hit fields
    #[serde(rename = "hittingPlayerId", default)]
    pub hitting_player_id: Option<i64>,
    #[serde(rename = "hitteePlayerId", default)]
    pub hittee_player_id: Option<i64>,

    // Blocked-shot fields
    #[serde(rename = "blockingPlayerId", default)]
    pub blocking_player_id: Option<i64>,

    // Penalties use `duration` and `descKey`, unlike several related NHL payloads.
    #[serde(rename = "typeCode", default)]
    pub type_code: Option<String>,
    #[serde(rename = "descKey", default)]
    pub desc_key: Option<String>,
    #[serde(default)]
    pub duration: Option<i16>,
    #[serde(rename = "committedByPlayerId", default)]
    pub committed_by_player_id: Option<i64>,
    #[serde(rename = "drawnByPlayerId", default)]
    pub drawn_by_player_id: Option<i64>,

    // Faceoff fields
    #[serde(rename = "winningPlayerId", default)]
    pub winning_player_id: Option<i64>,
    #[serde(rename = "losingPlayerId", default)]
    pub losing_player_id: Option<i64>,
}

// ── Game landing scoring summary ─────────────────────────────────────────────

/// Categorical manpower strength reported by the NHL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventStrength {
    Even,
    PowerPlay,
    ShortHanded,
}

impl EventStrength {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Even => "ev",
            Self::PowerPlay => "pp",
            Self::ShortHanded => "sh",
        }
    }

    pub(crate) fn from_nhl(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ev" => Some(Self::Even),
            "pp" => Some(Self::PowerPlay),
            "sh" => Some(Self::ShortHanded),
            _ => None,
        }
    }

    pub(crate) const fn inverted(self) -> Self {
        match self {
            Self::Even => Self::Even,
            Self::PowerPlay => Self::ShortHanded,
            Self::ShortHanded => Self::PowerPlay,
        }
    }
}

#[derive(Deserialize)]
struct GameLanding {
    #[serde(default)]
    summary: LandingSummary,
}

#[derive(Default, Deserialize)]
struct LandingSummary {
    #[serde(default)]
    scoring: Vec<LandingScoringPeriod>,
}

#[derive(Deserialize)]
struct LandingScoringPeriod {
    #[serde(default)]
    goals: Vec<LandingGoal>,
}

#[derive(Deserialize)]
struct LandingGoal {
    #[serde(rename = "eventId")]
    event_id: i32,
    strength: Option<String>,
}

// ── situationCode decode ──────────────────────────────────────────────────────

/// Decoded NHL `situationCode`.
///
/// The code is `[away_goalie][away_skaters][home_skaters][home_goalie]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SituationCode {
    pub away_goalie_present: bool,
    pub away_skater_count: i16,
    pub home_skater_count: i16,
    pub home_goalie_present: bool,
}

/// Decode an NHL situation code, rejecting malformed values.
pub fn decode_situation_code(code: &str) -> Option<SituationCode> {
    let [away_goalie, away_skaters, home_skaters, home_goalie] = code.as_bytes() else {
        return None;
    };
    if !matches!(away_goalie, b'0' | b'1')
        || !away_skaters.is_ascii_digit()
        || !home_skaters.is_ascii_digit()
        || !matches!(home_goalie, b'0' | b'1')
    {
        return None;
    }

    Some(SituationCode {
        away_goalie_present: *away_goalie == b'1',
        away_skater_count: i16::from(*away_skaters - b'0'),
        home_skater_count: i16::from(*home_skaters - b'0'),
        home_goalie_present: *home_goalie == b'1',
    })
}

/// Manpower state from the perspective of the team that owns the event.
pub fn strength_for_owner(
    situation: &SituationCode,
    owner_is_home: Option<bool>,
) -> Option<&'static str> {
    let home_effective = situation.home_skater_count - i16::from(!situation.home_goalie_present);
    let away_effective = situation.away_skater_count - i16::from(!situation.away_goalie_present);
    let difference = if owner_is_home? {
        home_effective - away_effective
    } else {
        away_effective - home_effective
    };

    Some(match difference.signum() {
        1 => "pp",
        -1 => "sh",
        _ => "ev",
    })
}

// ── Fetch ─────────────────────────────────────────────────────────────────────

/// Fetch play-by-play JSON for a single game and deserialize it.
///
/// Endpoint: <https://api-web.nhle.com/v1/gamecenter/{game_id}/play-by-play>
pub async fn fetch_play_by_play(game_id: i64) -> Result<PlayByPlay, AnyError> {
    let url = format!("https://api-web.nhle.com/v1/gamecenter/{game_id}/play-by-play");
    let json = fetch_api_json(&url).await?;
    let pbp: PlayByPlay = serde_json::from_str(&json)?;
    Ok(pbp)
}

/// Fetch explicit goal strength from the game landing scoring summary.
///
/// Historical play-by-play before 2009-10 omits `situationCode`, while this
/// endpoint still reports each goal as even-strength, power-play, or
/// short-handed using the same event ID.
pub async fn fetch_goal_strengths(game_id: i64) -> Result<HashMap<i32, EventStrength>, AnyError> {
    let url = format!("https://api-web.nhle.com/v1/gamecenter/{game_id}/landing");
    let json = fetch_api_json(&url).await?;
    let landing: GameLanding = serde_json::from_str(&json)?;

    Ok(landing
        .summary
        .scoring
        .into_iter()
        .flat_map(|period| period.goals)
        .filter_map(|goal| {
            let strength = EventStrength::from_nhl(goal.strength.as_deref()?)?;
            Some((goal.event_id, strength))
        })
        .collect())
}

/// Return whether the play-by-play needs scoring-summary enrichment for at
/// least one goal.
pub fn needs_goal_strengths(pbp: &PlayByPlay) -> bool {
    pbp.plays.iter().any(|play| {
        play.type_desc_key == "goal"
            && play
                .situation_code
                .as_deref()
                .and_then(decode_situation_code)
                .is_none()
    })
}

// ── Transform ─────────────────────────────────────────────────────────────────

/// Transform a PlayByPlay response into typed Db model vectors.
///
/// Single pass over pbp.plays — classifies each event by typeDescKey and
/// populates all seven return vectors.
///
/// Shootout events (periodType == "SO") are skipped per REQUIREMENTS.md.
/// Events with a recognized typeDescKey but missing details are skipped with a
/// warning string collected in the returned skip_warnings vector.
///
/// eventOwnerTeamId is translated from NHL team ID to franchise ID via
/// team_id_map. Unmapped team IDs store as NULL (nullable column).
///
/// Returns (events, goals, shots, hits, blocks, penalties, faceoffs, skip_warnings).
#[allow(clippy::type_complexity)]
pub fn transform_events(
    pbp: &PlayByPlay,
    team_id_map: &HashMap<i64, i64>,
) -> (
    Vec<DbEvent>,
    Vec<DbGoal>,
    Vec<DbShot>,
    Vec<DbHit>,
    Vec<DbBlock>,
    Vec<DbPenalty>,
    Vec<DbFaceoff>,
    Vec<String>,
) {
    transform_events_with_goal_strengths(pbp, team_id_map, &HashMap::new())
}

/// Transform play-by-play and enrich goals with explicit scoring-summary
/// strength when the play itself has no valid `situationCode`.
#[allow(clippy::type_complexity)]
pub fn transform_events_with_goal_strengths(
    pbp: &PlayByPlay,
    team_id_map: &HashMap<i64, i64>,
    goal_strengths: &HashMap<i32, EventStrength>,
) -> (
    Vec<DbEvent>,
    Vec<DbGoal>,
    Vec<DbShot>,
    Vec<DbHit>,
    Vec<DbBlock>,
    Vec<DbPenalty>,
    Vec<DbFaceoff>,
    Vec<String>,
) {
    transform_events_with_strength_sources(pbp, team_id_map, goal_strengths, &HashMap::new())
}

/// Transform play-by-play using explicit NHL strength sources in priority
/// order: situation code, structured scoring summary, then archived report.
#[allow(clippy::type_complexity)]
pub fn transform_events_with_strength_sources(
    pbp: &PlayByPlay,
    team_id_map: &HashMap<i64, i64>,
    goal_strengths: &HashMap<i32, EventStrength>,
    report_strengths: &HashMap<i32, EventStrength>,
) -> (
    Vec<DbEvent>,
    Vec<DbGoal>,
    Vec<DbShot>,
    Vec<DbHit>,
    Vec<DbBlock>,
    Vec<DbPenalty>,
    Vec<DbFaceoff>,
    Vec<String>,
) {
    let game_id = pbp.id;
    let mut events = Vec::new();
    let mut goals = Vec::new();
    let mut shots = Vec::new();
    let mut hits = Vec::new();
    let mut blocks = Vec::new();
    let mut penalties = Vec::new();
    let mut faceoffs = Vec::new();
    let mut skip_warnings = Vec::new();

    for play in &pbp.plays {
        // Skip shootout events — out of scope per REQUIREMENTS.md
        if play.period_descriptor.period_type == "SO" {
            continue;
        }

        let raw_owner_team_id = play.details.as_ref().and_then(|d| d.event_owner_team_id);
        let owner_is_home = match raw_owner_team_id {
            Some(team_id) if team_id == pbp.home_team.id => Some(true),
            Some(team_id) if team_id == pbp.away_team.id => Some(false),
            Some(team_id) => {
                skip_warnings.push(format!(
                    "event {} in game {} has owner team {} outside the game matchup",
                    play.event_id, game_id, team_id
                ));
                None
            }
            None => None,
        };

        let decoded = play
            .situation_code
            .as_deref()
            .and_then(decode_situation_code);
        let situation_code = decoded.and(play.situation_code.clone());
        let summary_strength = (play.type_desc_key == "goal")
            .then(|| goal_strengths.get(&play.event_id).copied())
            .flatten();
        let (strength, strength_source) = if let Some(situation) = decoded.as_ref() {
            (
                strength_for_owner(situation, owner_is_home).map(str::to_string),
                StrengthSource::SituationCode,
            )
        } else if let Some(summary_strength) = summary_strength {
            (
                Some(summary_strength.as_str().to_string()),
                StrengthSource::ScoringSummary,
            )
        } else if let Some(report_strength) = report_strengths.get(&play.event_id) {
            (
                Some(report_strength.as_str().to_string()),
                StrengthSource::HtmlReport,
            )
        } else {
            (None, StrengthSource::Unavailable)
        };

        // Translate event_owner_team_id from NHL team ID to franchise ID
        let event_owner_team_id: Option<i64> = play
            .details
            .as_ref()
            .and_then(|d| d.event_owner_team_id)
            .and_then(|tid| team_id_map.get(&tid).copied());

        // Extract coordinates and zone from details (all Optional)
        let (x_coord, y_coord, zone_code) = play.details.as_ref().map_or((None, None, None), |d| {
            (d.x_coord, d.y_coord, d.zone_code.clone())
        });

        let base = DbEvent {
            game_id,
            event_id_in_game: play.event_id,
            period: play.period_descriptor.number,
            period_type: play.period_descriptor.period_type.clone(),
            time_in_period: play.time_in_period.clone(),
            event_type: play.type_desc_key.clone(),
            x_coord,
            y_coord,
            zone_code,
            event_owner_team_id,
            away_goalie_present: decoded.map(|value| value.away_goalie_present),
            away_skater_count: decoded.map(|value| value.away_skater_count),
            home_skater_count: decoded.map(|value| value.home_skater_count),
            home_goalie_present: decoded.map(|value| value.home_goalie_present),
            strength,
            strength_source,
            situation_code,
        };
        events.push(base);

        // Classify into child type vectors
        match play.type_desc_key.as_str() {
            "goal" => {
                match &play.details {
                    None => {
                        skip_warnings.push(format!(
                            "skip: goal event {} in game {} has no details",
                            play.event_id, game_id
                        ));
                    }
                    Some(d) => {
                        goals.push(DbGoal {
                            event_id_in_game: play.event_id,
                            scorer_player_id: d.scoring_player_id,
                            assist1_player_id: d.assist1_player_id,
                            assist2_player_id: d.assist2_player_id,
                            goalie_id: d.goalie_in_net_id,
                            shot_type: d.shot_type.clone(),
                        });
                        // Goal payloads identify the shooter as `scoringPlayerId`,
                        // while shot payloads use `shootingPlayerId`.
                        shots.push(DbShot {
                            event_id_in_game: play.event_id,
                            shooting_player_id: d.scoring_player_id,
                            goalie_in_net_id: d.goalie_in_net_id,
                            shot_type: d.shot_type.clone(),
                        });
                    }
                }
            }
            "shot-on-goal" => match &play.details {
                None => {
                    skip_warnings.push(format!(
                        "skip: shot-on-goal event {} in game {} has no details",
                        play.event_id, game_id
                    ));
                }
                Some(d) => {
                    shots.push(DbShot {
                        event_id_in_game: play.event_id,
                        shooting_player_id: d.shooting_player_id,
                        goalie_in_net_id: d.goalie_in_net_id,
                        shot_type: d.shot_type.clone(),
                    });
                }
            },
            "hit" => match &play.details {
                None => {
                    skip_warnings.push(format!(
                        "skip: hit event {} in game {} has no details",
                        play.event_id, game_id
                    ));
                }
                Some(d) => {
                    hits.push(DbHit {
                        event_id_in_game: play.event_id,
                        hitting_player_id: d.hitting_player_id,
                        hittee_player_id: d.hittee_player_id,
                    });
                }
            },
            "blocked-shot" => match &play.details {
                None => {
                    skip_warnings.push(format!(
                        "skip: blocked-shot event {} in game {} has no details",
                        play.event_id, game_id
                    ));
                }
                Some(d) => {
                    blocks.push(DbBlock {
                        event_id_in_game: play.event_id,
                        blocking_player_id: d.blocking_player_id,
                        shooting_player_id: d.shooting_player_id,
                    });
                }
            },
            "penalty" => match &play.details {
                None => {
                    skip_warnings.push(format!(
                        "skip: penalty event {} in game {} has no details",
                        play.event_id, game_id
                    ));
                }
                Some(d) => {
                    penalties.push(DbPenalty {
                        event_id_in_game: play.event_id,
                        committed_by_player_id: d.committed_by_player_id,
                        drawn_by_player_id: d.drawn_by_player_id,
                        infraction_type: d.desc_key.clone(),
                        duration_minutes: d.duration,
                    });
                }
            },
            "faceoff" => match &play.details {
                None => {
                    skip_warnings.push(format!(
                        "skip: faceoff event {} in game {} has no details",
                        play.event_id, game_id
                    ));
                }
                Some(d) => {
                    faceoffs.push(DbFaceoff {
                        event_id_in_game: play.event_id,
                        winning_player_id: d.winning_player_id,
                        losing_player_id: d.losing_player_id,
                    });
                }
            },
            _ => {
                // Unknown or untracked event type (stoppage, period-start, missed-shot, etc.)
                // These are stored in the base events table but have no child record.
            }
        }
    }

    (
        events,
        goals,
        shots,
        hits,
        blocks,
        penalties,
        faceoffs,
        skip_warnings,
    )
}
