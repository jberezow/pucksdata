//! Long-lived daemon — interval-based sync loop with SIGTERM/Ctrl-C graceful shutdown.

/// Run periodic synchronization until SIGTERM or Ctrl-C is received.
pub async fn run_daemon(
    pool: &sqlx::PgPool,
    interval_secs: u64,
    backfill_on_start: bool,
) -> Result<(), crate::AnyError> {
    // Retaining the guard enforces a single daemon instance for this database.
    let _lock = crate::process::sync::acquire_daemon_lock(pool).await?;

    if backfill_on_start {
        crate::process::backfill::run_backfill(pool, None).await?;
    }

    // A slow sync should skip missed ticks rather than trigger burst catch-up.
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    run_loop(pool, interval_secs, &mut interval).await
}

#[cfg(unix)]
async fn run_loop(
    pool: &sqlx::PgPool,
    interval_secs: u64,
    interval: &mut tokio::time::Interval,
) -> Result<(), crate::AnyError> {
    // Signal tasks notify the loop, which cancels an active sync before exiting.
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let tx1 = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("received Ctrl-C — shutting down");
        tx1.send(true).ok();
    });

    let tx2 = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Ok(mut sigterm) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sigterm.recv().await;
            eprintln!("received SIGTERM — shutting down");
            tx2.send(true).ok();
        }
    });

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // If shutdown arrives mid-sync, drop the sync future. Syncs are
                // idempotent so aborting mid-way is safe.
                tokio::select! {
                    _ = tick_sync(pool, interval_secs) => {}
                    _ = shutdown_rx.changed() => {
                        eprintln!("sync interrupted — shutting down");
                        break;
                    }
                }
                if *shutdown_rx.borrow() {
                    break;
                }
            }

            _ = shutdown_rx.changed() => {
                break;
            }
        }
    }

    Ok(())
}

#[cfg(not(unix))]
async fn run_loop(
    pool: &sqlx::PgPool,
    interval_secs: u64,
    interval: &mut tokio::time::Interval,
) -> Result<(), crate::AnyError> {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let tx1 = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("received Ctrl-C — shutting down");
        tx1.send(true).ok();
    });

    loop {
        tokio::select! {
            _ = interval.tick() => {
                tokio::select! {
                    _ = tick_sync(pool, interval_secs) => {}
                    _ = shutdown_rx.changed() => {
                        eprintln!("sync interrupted — shutting down");
                        break;
                    }
                }
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = shutdown_rx.changed() => {
                break;
            }
        }
    }

    Ok(())
}

/// Execute one sync tick, leaving failures for the next interval to retry.
async fn tick_sync(pool: &sqlx::PgPool, interval_secs: u64) {
    eprintln!(
        "[{}] starting sync",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
    );

    match crate::process::sync::run_sync(pool, None).await {
        Ok(_summary) => {
            let next = chrono::Utc::now() + chrono::Duration::seconds(interval_secs as i64);
            eprintln!("next sync at {} UTC", next.format("%H:%M"));
        }
        Err(e) => {
            eprintln!("sync failed, continuing: {e}");
        }
    }
}
