use axum::{
    Json, Router, extract::State, response::IntoResponse, routing::{get, post}
};
use clap::{Parser}; // for cli
use dashmap::DashMap;
use lazy_static::lazy_static;
use prometheus::{
    Counter, Encoder, Gauge, Histogram, TextEncoder, register_counter, register_gauge,
    register_histogram,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

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
    rate_window: u64,
}

// remvooing constatnt as they now will come from CLI directly

// Defining metrics globally using lazy_static
lazy_static! {
    static ref REQUEST_TOTAL: Counter =
        register_counter!("ollama_requests_total", "Total number of requests").unwrap();
    static ref CACHE_HITS: Counter =
        register_counter!("ollama_cache_hits_total", "Total cache hits").unwrap();
    static ref CACHE_MISSES: Counter =
        register_counter!("ollama_caches_misses_total", "Total cache misses").unwrap();
    static ref REQUEST_LATENCY: Histogram = register_histogram!(
        "ollama_request_latency_seconds",
        "Request latency in seconds"
    )
    .unwrap();
    static ref CACHE_SIZE: Gauge =
        register_gauge!("ollama_cache_size", "Current number of items in cache").unwrap();
}

// cache entry with timestamp
#[derive(Clone)]
struct CacheEntry {
    response: String,
    created_at: Instant,
}
//Rate limimt entry
struct RateLimitEntry {
    count: u32,
    window_start: Instant,
}

// Batched request - holds request + response channel
struct BatchedRequest {
    request: GenerateRequest,
    response_tx: oneshot::Sender<Result<GenerateResponse, String>>,
}

// app's shared state
struct AppState {
    client: reqwest::Client,
    cache: DashMap<String, CacheEntry>, // String -> CacheEntry
    ttl: Duration,                      // how long cache will be valid
    ollama_url: String,
    rate_limiter: DashMap<String, RateLimitEntry>,
    rate_limit: u32,       // max request allowed
    rate_window: Duration, // Duration of rate limit
    batch_tx: mpsc::Sender<BatchedRequest>,
}

// this is main async function with tokio
#[tokio::main]
async fn main() {
    // parse cli arguments
    let args = Args::parse();
    let (batch_tx, batch_rx) = mpsc::channel::<BatchedRequest>(100);

    // creating shared state
    let state = Arc::new(AppState {
        client: reqwest::Client::new(),
        cache: DashMap::new(),
        ttl: Duration::from_secs(args.cache_ttl),
        ollama_url: args.ollama_url.clone(),
        rate_limiter: DashMap::new(),
        rate_limit: args.rate_limit,
        rate_window: Duration::from_secs(args.rate_window),
        batch_tx,
    });

    // spawn the background worker
    let worker_client = reqwest::Client::new();
    let worker_url = args.ollama_url.clone();
    let worker_cache = state.cache.clone();
    let worker_ttl = state.ttl;

    tokio::spawn(async move {
        batch_worker(batch_rx, worker_client, worker_url, worker_cache, worker_ttl).await;
    });

    //creating the router with rooutes
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/api/generate", post(generate_handler)) // post route
        .route("/metrics", get(metrics_handler)) // metrics endpoint
        .with_state(state); // put client in state

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    println!("Gateway running on http://localhost:{}", args.port);
    println!("Forwarding to Ollama at {}", args.ollama_url);
    println!("Cache TTL: {} seconds", args.cache_ttl);
    println!(
        "Rate limit: {} requests per {} seconds",
        args.rate_limit, args.rate_window
    );
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

    let mut entry = state
        .rate_limiter
        .entry(ip.to_string())
        .or_insert(RateLimitEntry {
            count: 0,
            window_start: now,
        });

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

// Background worker -> processes requests from queue one by one

async fn batch_worker(
    mut rx: mpsc::Receiver<BatchedRequest>,
    client: reqwest::Client,
    ollama_url: String,
    cache: DashMap<String, CacheEntry>,
    ttl: Duration,
) {
    println!("Batch worker started - processing requests sequentially");

    // keep receiving the requests from queue
    while let Some(batched_req) = rx.recv().await {
        let cache_key = make_cache_key(&batched_req.request);

        // check cache first
        if let Some(entry) = cache.get(&cache_key) {
            if entry.created_at.elapsed() < ttl {
                CACHE_HITS.inc();
                println!("[Worker] Cache HIT");
                if let Ok(response) = serde_json::from_str(&entry.response) {
                    let _ = batched_req.response_tx.send(Ok(response));
                    continue;
                }
            }
        }
        CACHE_MISSES.inc();
        println!("[Worker] Cache MISS - calling Ollama");

        // Call ollama
        let result = client
        .post(format!("{}/api/generate", ollama_url))
        .json(&batched_req.request)
        .send()
        .await;

        let response = match result {
            Ok(res) => {
                match res.json::<GenerateResponse>().await {
                    Ok(body) => {
                        // saving to cache
                        if let Ok(json) = serde_json::to_string(&body) {
                            cache.insert(cache_key, CacheEntry {
                                response: json,
                                created_at: Instant::now(),
                            });
                            CACHE_SIZE.set(cache.len() as f64); 
                        }
                        Ok(body)
                    }
                    Err(e) => Err(format!("Parse Error: {}", e))
                }
            }
            Err(e) => Err(format!("Request failed: {}", e))
        };
        // Send response back to handler
        let _ = batched_req.response_tx.send(response);
    }
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

    // Create oneshot channel for response
    let (response_tx, response_rx) = oneshot::channel();

    let batched = BatchedRequest {
        request: payload,
        response_tx,
    };
    state.batch_tx.send(batched).await
    .map_err(|_| "Failed to queue request".to_string())?;

    // wait for response from worker
    let result = response_rx.await
    .map_err(|_| "worker failed to respond".to_string())?;

    REQUEST_LATENCY.observe(start_time.elapsed().as_secs_f64());

    result.map(Json)
}
