mod config;
mod models;
mod metrics;
mod cache;
mod rate_limit;
mod state;
mod worker;
mod handlers;
mod load_balancer;

use config::Args;
use state::AppState;
use models::BatchedRequest;
use worker::batch_worker;
use handlers::{health_handler, generate_handler, metrics_handler};
use load_balancer::{LoadBalancer, health_checker};

use axum::{Router, routing::{get, post}};
use clap::Parser;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use std::time::Duration;

// this is main async function with tokio
#[tokio::main]
async fn main() {
    // parse cli arguments
    let args = Args::parse();
    let (batch_tx, batch_rx) = mpsc::channel::<BatchedRequest>(100);


    let load_balancer = Arc::new(LoadBalancer::new(&args.backends));
    // creating shared state
    let state = Arc::new(AppState {
        client: reqwest::Client::new(),
        cache: DashMap::new(),
        ttl: Duration::from_secs(args.cache_ttl),
        load_balancer: Arc::clone(&load_balancer),
        rate_limiter: DashMap::new(),
        rate_limit: args.rate_limit,
        rate_window: Duration::from_secs(args.rate_window),
        batch_tx,
    });

    // SPWAN health checker
    let health_lb = Arc::clone(&load_balancer);
    let health_client = reqwest::Client::new();
    let health_interval = Duration::from_secs(args.health_interval);
    tokio::spawn(async move {
        health_checker(health_lb, health_client, health_interval).await;
    });

    // spawn the background worker
    let worker_client = reqwest::Client::new();
    let worker_lob = Arc::clone(&load_balancer);
    let worker_cache = state.cache.clone();
    let worker_ttl = state.ttl;

    tokio::spawn(async move {
        batch_worker(batch_rx, worker_client, worker_lob, worker_cache, worker_ttl).await;
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
    println!("Backends: {}", args.backends);
    println!("Cache TTL: {} seconds", args.cache_ttl);
    println!(
        "Rate limit: {} requests per {} seconds",
        args.rate_limit, args.rate_window
    );
    axum::serve(listener, app).await.unwrap();
}
