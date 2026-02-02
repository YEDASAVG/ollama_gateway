use std::time::Instant;

// Rate limit entry - tracks requests per IP/key
pub struct RateLimitEntry {
    pub count: u32,
    pub window_start: Instant,
}