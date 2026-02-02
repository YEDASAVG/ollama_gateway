use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use crate::cache::CacheEntry;
use crate::load_balancer::{LoadBalancer};
use crate::models::{BatchedRequest, GenerateResponse};
use crate::cache::make_cache_key;
use crate::metrics::{CACHE_HITS, CACHE_MISSES, CACHE_SIZE};


pub async fn batch_worker(
    mut rx: mpsc::Receiver<BatchedRequest>,
    client: reqwest::Client,
    load_balancer: Arc<LoadBalancer>,
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

        let backend = match load_balancer.get_backend() {
            Some(b) => b,
            None => {
                let _ = batched_req.response_tx.send(Err("No Healthy backends available".to_string()));
                continue;
            }
        };
        println!("[Worker] Using Backend: {}", backend.url);

        // Call ollama
        let result = client
        .post(format!("{}/api/generate", backend.url)) // use backend.url
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
            // Marking backend as unhelathy on error
            Err(e) => {
                backend.set_healthy(false);
                println!("[Worker] Backend {} failed, marked unhealthy", backend.url);
                Err(format!("Request failed: {}", e))
            }
        };
        // Send response back to handler
        let _ = batched_req.response_tx.send(response);
    }
}