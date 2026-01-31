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
use clap::Parser; // for cli


//cli argument structure
#[derive(Parser, Debug)]
#[command(name = "ollama gateway")]
#[command(about = "High performance caching proxy for ollama")]
struct Args {
    //port to run the server on
    #[arg(short, long, default_value_t = 8080)]
    port: u16,
    
    //ollama server url
    #[arg(short, long, default_value = "http://localhost:11434")]
    ollama_url: String,
    
    //Cache TTL in seconds
    #[arg(short, long, default_value_t = 30)]
    cache_ttl: u64,
    
    //Rate limit max req per window
    #[arg(long, default_value_t = 10)]
    rate_limit: u32,
    
    //Rate limit window in seconds
    #[arg(long, default_value_t = 60)]
    rate_window: u64
}

// remvooing constatnt as they now will come from CLI directly

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
//Rate limimt entry
struct RateLimitEntry {
    count: u32,
    window_start: Instant,
}
// app's shared state
struct AppState {
    client: reqwest::Client,
    cache: DashMap<String, CacheEntry>, // String -> CacheEntry
    ttl: Duration,                      // how long cache will be valid
    ollama_url: String,
    rate_limiter: DashMap<String, RateLimitEntry>,
    rate_limit: u32, // max request allowed
    rate_window: Duration, // Duration of rate limit
}

// this is main async function with tokio
#[tokio::main]
async fn main() {
    // parse cli arguments
    let args = Args::parse();

    // creating shared state
    let state = Arc::new(AppState {
        client: reqwest::Client::new(),
        cache: DashMap::new(),
        ttl: Duration::from_secs(args.cache_ttl),
        ollama_url: args.ollama_url.clone(),
        rate_limiter: DashMap::new(),
        rate_limit: args.rate_limit,
        rate_window: Duration::from_secs(args.rate_window),
    });

    //creating the router with rooutes
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/api/generate", post(generate_handler)) // post route
        .route("/metrics", get(metrics_handler))// metrics endpoint
        .with_state(state); // put client in state

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    println!("Gateway running on http://localhost:{}", args.port);
    println!("Forwarding to Ollama at {}", args.ollama_url);
    println!("Cache TTL: {} seconds", args.cache_ttl);
    println!("Rate limit: {} requests per {} seconds", args.rate_limit, args.rate_window);
    axum::serve(listener, app).await.unwrap();
}

async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
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
// health handler
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

// rate limimt function
fn check_rate_limit(state: &AppState, ip: &str) -> bool {
    let now = Instant::now();

    let mut entry = state.rate_limiter
    .entry(ip.to_string())
    .or_insert(RateLimitEntry { count: 0, window_start: now, });

    //windows expired..? Reset it
    if entry.window_start.elapsed() > state.rate_window {
        entry.count = 1;
        entry.window_start = now;
        return true;
    }

    // under limit.? Allow
    if entry.count < state.rate_limit {
        entry.count += 1;
        return true;
    }

    //over limit
    false
}

//post handler
async fn generate_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateRequest>,
) -> Result<Json<GenerateResponse>, String> {

    //track request
    REQUEST_TOTAL.inc();
    if !check_rate_limit(&state, "global") {
        return Err("Rate limit exceeded. Try again later.".to_string());
    }
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
        .post(format!("{}/api/generate", state.ollama_url))
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
