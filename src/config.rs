use clap::Parser;

// CLI argument structure
#[derive(Parser, Debug, Clone)]
#[command(name = "ollama-gateway")]
#[command(about = "High performance caching proxy for Ollama")]
pub struct Args {
    // Port to run the server on
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    // Backend servers (comma-separated)
    // Example: "localhost:11434,localhost:11435"
    #[arg(short, long, default_value = "localhost:11434")]
    pub backends: String,

    // Cache TTL in seconds
    #[arg(short, long, default_value_t = 30)]
    pub cache_ttl: u64,

    // Rate limit max requests per window
    #[arg(long, default_value_t = 10)]
    pub rate_limit: u32,

    // Rate limit window in seconds
    #[arg(long, default_value_t = 60)]
    pub rate_window: u64,

    // Health check interval
    #[arg(long, default_value_t = 30)]
    pub health_interval: u64
}