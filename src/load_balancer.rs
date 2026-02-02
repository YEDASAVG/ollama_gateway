use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::time::{Duration, interval};



// Single Backend server

pub struct Backend {
    pub url: String,
    pub helathy: AtomicBool, // is it owrking..?
}

impl Backend {
    pub fn new(url: String) -> Self {
        Self {
            url,
            helathy: AtomicBool::new(true),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.helathy.load(Ordering::Relaxed)
    }

    pub fn set_healthy(&self, healthy: bool) {
        self.helathy.store(healthy, Ordering::Relaxed);
    }
}

// Load Balancer wiht multiple backends

pub struct LoadBalancer {
    pub backends: Vec<Arc<Backend>>,
    current: AtomicUsize,
}

impl LoadBalancer {
    // Create from comma-seprated urls "localhst::11434, localhost::11435"
    pub fn new(backends_str: &str) -> Self {
        let backends: Vec<Arc<Backend>> = backends_str
            .split(',') 
            .map(|s| s.trim())// remove spaces
            .filter(|s| !s.is_empty())// remove empty strings
            .map(|url| {
                // add http:// if not present
                let full_url = if url.starts_with("http") {
                    url.to_string()
                } else {
                    format!("http://{}", url)
                };
                Arc::new(Backend::new(full_url))
            })
            .collect();
        if backends.is_empty() {
            panic!("At least one backend required");
        }

        println!(
            "Load balancer initialized with {} backends:",
            backends.len()
        );
        for (i, b) in backends.iter().enumerate() {
            println!(".  [{}]  {}", i + 1, b.url);
        }

        Self {
            backends,
            current: AtomicUsize::new(0),
        }
    }

    // Get next healthy backend (round-robin)
    pub fn get_backend(&self) -> Option<Arc<Backend>> {
        let len = self.backends.len();
        let start = self.current.fetch_add(1, Ordering::Relaxed) % len;

        for i in 0..len {
            let idx = (start + i) % len;
            let backend = &self.backends[idx];

            if backend.is_healthy() {
                return Some(Arc::clone(backend));
            }
        }
        // No healthy backends
        None
    }

    // Get all backedns (for health checker0)
    pub fn all_backends(&self) -> &Vec<Arc<Backend>> {
        &self.backends
    }
}

// Health check functin - runs every 30 seconds

pub async fn health_checker(
    load_balancer: Arc<LoadBalancer>,
    client: reqwest::Client,
    check_interval: Duration,
) {
    let mut interval = interval(check_interval);

    println!("Health checker started (interval: {:?}", check_interval);

    loop {
        interval.tick().await;

        for backend in load_balancer.all_backends() {
            let url = format!("{}/api/tags", backend.url); // Ollama health endpoint

            let was_healthy = backend.is_healthy();

            let is_healthy = match client.get(&url).timeout(Duration::from_secs(5)).send().await {
                Ok(res) => res.status().is_success(),
                Err(_) => false,
            };
            backend.set_healthy(is_healthy);

            // Log status changes
            if was_healthy != is_healthy {
                if is_healthy {
                    println!("Backend {} is now Healthy", backend.url);
                } else {
                    println!("Backend {} is now Unhealthy", backend.url);
                }
            }
        }
    }
}

















