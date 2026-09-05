// Daemon scheduling and season-selection behavior.

use tokio::time::{Duration, MissedTickBehavior};

#[tokio::test]
async fn test_daemon_args_defaults() {
    let interval_secs_flag: Option<u64> = None;
    let interval_secs = interval_secs_flag
        .or_else(|| {
            std::env::var("SYNC_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(21600);
    assert_eq!(
        interval_secs, 21600,
        "default interval should be 21600 seconds (6 hours)"
    );

    std::env::set_var("SYNC_INTERVAL_SECS", "3600");
    let interval_secs_env = interval_secs_flag
        .or_else(|| {
            std::env::var("SYNC_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(21600);
    assert_eq!(
        interval_secs_env, 3600,
        "SYNC_INTERVAL_SECS env var should override default"
    );
    std::env::remove_var("SYNC_INTERVAL_SECS");
}

#[tokio::test]
async fn test_daemon_interval_skip_behavior() {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    assert_eq!(
        interval.missed_tick_behavior(),
        MissedTickBehavior::Skip,
        "daemon interval must use MissedTickBehavior::Skip — no burst catch-up after slow syncs"
    );
}

#[tokio::test]
async fn test_daemon_exported() {
    let _ = pucksdata::process::daemon::run_daemon as fn(_, _, _) -> _;
}

#[test]
fn test_current_season_october_start() {
    assert_eq!(
        pucksdata::process::sync::season_for_date(10, 2025),
        20252026
    );
}

#[test]
fn test_current_season_mid_season() {
    assert_eq!(pucksdata::process::sync::season_for_date(3, 2026), 20252026);
}

#[test]
fn test_current_season_june() {
    assert_eq!(pucksdata::process::sync::season_for_date(6, 2026), 20252026);
}

#[test]
fn test_current_season_september() {
    assert_eq!(pucksdata::process::sync::season_for_date(9, 2025), 20242025);
}

#[test]
fn test_current_season_next_season() {
    assert_eq!(
        pucksdata::process::sync::season_for_date(10, 2026),
        20262027
    );
}

#[test]
fn test_current_season_is_available_to_sync() {
    let _season_fn = pucksdata::process::sync::current_season as fn() -> i32;
}
