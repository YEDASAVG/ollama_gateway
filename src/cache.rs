use sha2::{Digest, Sha256};
use std::time::Instant;
use crate::models::GenerateRequest;

// Cache entry with timestamp
#[derive(Clone)]
pub struct CacheEntry {
    pub response: String,
    pub created_at: Instant,
}

// Create a cache key (hash of model + prompt)
pub fn make_cache_key(req: &GenerateRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(&req.model);
    hasher.update(&req.prompt);
    format!("{:x}", hasher.finalize())
}