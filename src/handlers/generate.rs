use axum::{Json, extract::State};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::oneshot;
use crate::state::AppState;
use crate::models::{GenerateRequest, GenerateResponse, BatchedRequest};
use crate::metrics::{REQUEST_TOTAL, REQUEST_LATENCY};

// Rate limit check function
fn check_rate_limit(state: &AppState, ip: &str) -> bool {
    let now = Instant::now();

    let mut entry = state
        .rate_limiter
        .entry(ip.to_string())
        .or_insert(crate::rate_limit::RateLimitEntry {
            count: 0,
            window_start: now,
        });

    if entry.window_start.elapsed() > state.rate_window {
        entry.count = 1;
        entry.window_start = now;
        return true;
    }

    if entry.count < state.rate_limit {
        entry.count += 1;
        return true;
    }

    false
}

pub async fn generate_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateRequest>,
) -> Result<Json<GenerateResponse>, String> {
    REQUEST_TOTAL.inc();

    if !check_rate_limit(&state, "global") {
        return Err("Rate limit exceeded. Try again later.".to_string());
    }

    let start_time = Instant::now();

    let (response_tx, response_rx) = oneshot::channel();

    let batched = BatchedRequest {
        request: payload,
        response_tx,
    };

    state.batch_tx.send(batched).await
        .map_err(|_| "Failed to queue request".to_string())?;

    let result = response_rx.await
        .map_err(|_| "Worker failed to respond".to_string())?;

    REQUEST_LATENCY.observe(start_time.elapsed().as_secs_f64());

    result.map(Json)
}