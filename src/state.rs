use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use crate::cache::CacheEntry;
use crate::rate_limit::RateLimitEntry;
use crate::models::BatchedRequest;
use crate::load_balancer::LoadBalancer;
// app's shared state

pub struct AppState {
    pub client: reqwest::Client,
    pub cache: DashMap<String, CacheEntry>, // String -> CacheEntry
    pub ttl: Duration,                      // how long cache will be valid
    pub load_balancer: Arc<LoadBalancer>,
    pub rate_limiter: DashMap<String, RateLimitEntry>,
    pub rate_limit: u32,       // max request allowed
    pub rate_window: Duration, // Duration of rate limit
    pub batch_tx: mpsc::Sender<BatchedRequest>,
}