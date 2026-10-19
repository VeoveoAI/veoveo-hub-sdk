//! Deterministic policy timing, separate from real HTTP scheduling.
use super::*;

#[tokio::test(start_paused = true)]
async fn rapid_ends_back_off_and_only_observed_listening_resets_the_delay() {
    let mut recovery = SubscriptionRecovery::default();
    for milliseconds in [100, 200, 400, 800, 1600, 3200, 5000, 5000] {
        let started = tokio::time::Instant::now();
        recovery.after_end().await;
        assert_eq!(started.elapsed(), Duration::from_millis(milliseconds));
    }
    // Time outside next(), including a slow snapshot, is not listening evidence.
    tokio::time::advance(Duration::from_secs(60)).await;
    let started = tokio::time::Instant::now();
    recovery.after_end().await;
    assert_eq!(started.elapsed(), Duration::from_secs(5));
    recovery.listening = Duration::from_secs(5);
    let started = tokio::time::Instant::now();
    recovery.after_end().await;
    assert_eq!(started.elapsed(), Duration::from_millis(100));
}

#[tokio::test(start_paused = true)]
async fn caller_deadline_and_drop_cancel_pending_recovery() {
    let mut recovery = SubscriptionRecovery::default();
    let started = tokio::time::Instant::now();
    let result = within(Some(Duration::from_millis(50)), async {
        recovery.after_end().await;
        Ok(())
    })
    .await;
    assert!(matches!(result, Err(CallError::Timeout(_))));
    assert_eq!(started.elapsed(), Duration::from_millis(50));
    let pending = tokio::spawn(async move { recovery.after_end().await });
    tokio::task::yield_now().await;
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
}
