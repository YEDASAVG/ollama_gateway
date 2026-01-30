use axum::{
    Json, Router, extract::State, response::IntoResponse, routing::{get, post}
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use prometheus::{Counter, Histogram, Gauge, Encoder, TextEncoder, register_counter, register_histogram, register_gauge};
use lazy_static::lazy_static;


// ollama's constant URL
const OLLAMA_URL: &str = "http://localhost:11434";
const CACHE_TTL_SECONDS: u64 = 360;

// Defining metrics globally using lazy_static
lazy_static! {
    static ref REQUEST_TOTAL: Counter = register_counter!(
        "ollama_requests_total",
        "Total number of requests"
    ).unwrap();

    static ref CACHE_HITS: Counter = register_counter!(
        "ollama_cache_hits_total",
        "Total cache hits"
    ).unwrap();

    static ref CACHE_MISSES: Counter = register_counter!(
        "ollama_caches_misses_total",
        "Total cache misses"
    ).unwrap();

    static ref REQUEST_LATENCY: Histogram = register_histogram!(
        "ollama_request_latency_seconds",
        "Request latency in seconds"
    ).unwrap();

    static ref CACHE_SIZE: Gauge = register_gauge!(
        "ollama_cache_size",
        "Current number of items in cache"
    ).unwrap();
}

// cache entry with timestamp
struct CacheEntry {
    response: String,
    created_at: Instant,
}
// app's shared state
struct AppState {
    client: reqwest::Client,
    cache: DashMap<String, CacheEntry>, // String -> CacheEntry
    ttl: Duration,                      // how long cache will be valid
}

// this is main async function with tokio
#[tokio::main]
async fn main() {
    // creating shared state
    let state = Arc::new(AppState {
        client: reqwest::Client::new(),
        cache: DashMap::new(),
        ttl: Duration::from_secs(CACHE_TTL_SECONDS),
    });

    //creating the router with rooutes
    let app = Router::new()
        .route("/hello", get(hello_handler))
        .route("/api/generate", post(generate_handler)) // post route
        .route("/metrics", get(metrics_handler))// metrics endpoint
        .with_state(state); // put client in state

    // start the server on port 8080
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    println!("Gateway running on http://localhost:8080");
    println!("Forwarding to Ollama at {}", OLLAMA_URL);
    axum::serve(listener, app).await.unwrap();
}

async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

async fn hello_handler() -> &'static str {
    "Hello, world!"
}

// Ollama API request format
#[derive(Deserialize, Serialize, Clone)]
struct GenerateRequest {
    model: String,
    prompt: String,
    #[serde(default)]
    stream: bool,
}

//ollama API response format
#[derive(Deserialize, Serialize, Clone)]
struct GenerateResponse {
    model: String,
    response: String,
}

// create a cache key (to hash model name + prompt)
fn make_cache_key(req: &GenerateRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(&req.model);
    hasher.update(&req.prompt);
    format!("{:x}", hasher.finalize())
}

//post handler
async fn generate_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateRequest>,
) -> Result<Json<GenerateResponse>, String> {

    //track request
    REQUEST_TOTAL.inc();
    let start_time = Instant::now();
    // create a cache key
    let cache_key = make_cache_key(&payload);

    // check in Cache
    if let Some(entry) = state.cache.get(&cache_key) {
        // check if cache expired or not
        if entry.created_at.elapsed() < state.ttl {
            CACHE_HITS.inc(); // tracking cache hits which was being done in terminal before
            println!("Cache HIT");
            let response: GenerateResponse = serde_json::from_str(&entry.response)
                .map_err(|e| format!("Cache parse error: {}", e))?;

            REQUEST_LATENCY.observe(start_time.elapsed().as_secs_f64()); // Track latency
            return Ok(Json(response));
        } else {
            // if expired remove from cache
            println!("Cache expired");
            drop(entry); // Release the lock before removing
            state.cache.remove(&cache_key);
            CACHE_SIZE.set(state.cache.len() as f64); //update cache size
        }
    }
    CACHE_MISSES.inc(); // track the cache miss
    println!("Cache MISS");

    // send request to ollama
    let ollama_response = state
        .client
        .post(format!("{}/api/generate", OLLAMA_URL))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to reach Ollama {}", e))?;

    // parse the response came from ollama
    let response_body: GenerateResponse = ollama_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse the Ollama response {}", e))?;

    // save it in Cache
    let response_json =
        serde_json::to_string(&response_body).map_err(|e| format!("Serialize error: {}", e))?;
    state.cache.insert(
        cache_key,
        CacheEntry {
            response: response_json,
            created_at: Instant::now(),
        },
    );
    CACHE_SIZE.set(state.cache.len() as f64); // update cache size
    REQUEST_LATENCY.observe(start_time.elapsed().as_secs_f64()); //track latency

    Ok(Json(response_body))
}
