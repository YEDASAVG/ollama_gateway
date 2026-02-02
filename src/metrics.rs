use lazy_static::lazy_static;
use prometheus::{Counter, Gauge, Histogram, register_counter, register_gauge, register_histogram};


lazy_static! {
    pub static ref REQUEST_TOTAL: Counter =
        register_counter!("ollama_requests_total", "Total number of requests").unwrap();
    pub static ref CACHE_HITS: Counter =
        register_counter!("ollama_cache_hits_total", "Total cache hits").unwrap();
    pub static ref CACHE_MISSES: Counter =
        register_counter!("ollama_cache_misses_total", "Total cache misses").unwrap();
    pub static ref REQUEST_LATENCY: Histogram = register_histogram!(
        "ollama_request_latency_seconds",
        "Request latency in seconds"
    )
    .unwrap();
    pub static ref CACHE_SIZE: Gauge =
        register_gauge!("ollama_cache_size", "Current number of items in cache").unwrap();
}