# Ollama Gateway

A high-performance caching proxy for Ollama that makes your local LLM faster and more reliable.

## What is This?

Ollama Gateway sits between your application and Ollama. It adds features that Ollama does not have out of the box:

- **Caching**: Same question asked twice? Get instant response from cache instead of waiting for LLM.
- **Request Batching**: Requests are queued and processed efficiently by a background worker.
- **Metrics**: See how many requests, cache hits, latency - all in Prometheus format.
- **Rate Limiting**: Protect your Ollama server from too many requests.
- **Load Balancing**: Distribute requests across multiple Ollama servers.
- **Health Checks**: Automatically detect and skip unhealthy backends.

```
Without Gateway:
User -> Ollama (every request processed, even duplicates)

With Gateway:
User -> Gateway -> Ollama (duplicates served from cache)
```

## Why Use This?

| Problem | Solution |
|---------|----------|
| Same prompts take same time every time | Cache returns instant responses for repeated queries |
| Multiple requests flood Ollama | Request batching queues and processes them orderly |
| No way to monitor Ollama usage | Prometheus metrics track everything |
| Ollama can get overloaded | Rate limiting protects your server |
| Single Ollama = single point of failure | Load balancing across multiple backends |

## Performance

| Scenario | Response Time | Notes |
|----------|---------------|-------|
| Without cache (Ollama call) | ~10-20 seconds | Full LLM inference |
| With cache (HIT) | ~0.009 seconds | Instant from memory |
| **Speedup** | **~2000x faster** | For repeated queries |

Real-world impact: If you ask the same question 100 times, you wait once and get 99 instant responses.

## Quick Start

### Prerequisites

- Rust (1.70 or later)
- Ollama running locally (default: localhost:11434)

### Installation

```bash
git clone https://github.com/YEDASAVG/ollama_gateway.git
cd ollama_gateway/ollama-gateway

cargo build --release
```

### Basic Usage

1. Start the gateway:
```bash
cargo run
```

2. Send requests to the gateway instead of Ollama:
```bash
curl -X POST http://localhost:8080/api/generate \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.2:1b", "prompt": "What is Rust?", "stream": false}'
```

The gateway forwards your request to Ollama and caches the response. Next time you ask the same question, you get an instant response.

## Configuration

All settings are passed as command line arguments.

### Available Options

| Option | Default | Description |
|--------|---------|-------------|
| `--port` or `-p` | 8080 | Port to run the gateway on |
| `--backends` or `-b` | localhost:11434 | Ollama server(s), comma-separated |
| `--cache-ttl` or `-c` | 30 | How long to keep cached responses (seconds) |
| `--rate-limit` | 10 | Maximum requests allowed per window |
| `--rate-window` | 60 | Rate limit window duration (seconds) |
| `--health-interval` | 30 | How often to check backend health (seconds) |

### Examples

Run on different port:
```bash
cargo run -- --port 3000
```

Connect to remote Ollama:
```bash
cargo run -- --backends "192.168.1.100:11434"
```

Multiple Ollama backends (load balancing):
```bash
cargo run -- --backends "localhost:11434,localhost:11435"
```

Longer cache duration (1 hour):
```bash
cargo run -- --cache-ttl 3600
```

Higher rate limit (100 requests per minute):
```bash
cargo run -- --rate-limit 100 --rate-window 60
```

## API Endpoints

### POST /api/generate

Forward a generation request to Ollama.

Request:
```json
{
  "model": "llama3.2:1b",
  "prompt": "What is Rust?",
  "stream": false
}
```

Response:
```json
{
  "model": "llama3.2:1b",
  "response": "Rust is a systems programming language..."
}
```

Note: Streaming (`stream: true`) is not yet supported.

### GET /health

Check if the gateway is running.

Response:
```json
{
  "status": "healthy",
  "timestamp": "2026-02-02T16:47:16.414524+00:00"
}
```

### GET /metrics

Prometheus-compatible metrics endpoint.

Response:
```
# HELP ollama_requests_total Total number of requests
# TYPE ollama_requests_total counter
ollama_requests_total 150

# HELP ollama_cache_hits_total Total cache hits
# TYPE ollama_cache_hits_total counter
ollama_cache_hits_total 45

# HELP ollama_cache_misses_total Total cache misses
# TYPE ollama_cache_misses_total counter
ollama_cache_misses_total 105

# HELP ollama_cache_size Current number of items in cache
# TYPE ollama_cache_size gauge
ollama_cache_size 42

# HELP ollama_request_latency_seconds Request latency in seconds
# TYPE ollama_request_latency_seconds histogram
ollama_request_latency_seconds_bucket{le="0.5"} 120
...
```

## Features Explained

### Caching

When you send a request, the gateway creates a unique key based on the model name and prompt. If the same request comes again within the TTL (time-to-live), the cached response is returned immediately.

How it works:
1. Request comes in: `model=llama3.2:1b, prompt="What is Rust?"`
2. Gateway creates hash: `sha256(model + prompt)` = `abc123...`
3. Gateway checks cache for `abc123...`
4. Cache miss: Forward to Ollama, store response, return to user
5. Same request again: Cache hit, return stored response instantly

Cache key is an exact match. "What is Rust?" and "what is rust?" are different keys.

### Rate Limiting

Protects your Ollama server from being overwhelmed. Default: 10 requests per 60 seconds.

When limit is exceeded:
```
Rate limit exceeded. Try again later.
```

Rate limit resets after the window duration passes.

### Load Balancing

If you have multiple Ollama servers, the gateway distributes requests using round-robin:

```
Request 1 -> Server A
Request 2 -> Server B
Request 3 -> Server A
Request 4 -> Server B
...
```

If a server becomes unhealthy (fails health check), it is automatically skipped until it recovers.

### Request Batching

Instead of handling each request directly in the HTTP handler, requests are queued and processed by a background worker. This provides:

1. **Non-blocking handlers**: HTTP handlers return quickly after queuing
2. **Controlled concurrency**: One worker processes requests in order
3. **Clean separation**: Network logic lives in the worker, not the handler

How it works:
```
User Request
     |
     v
[HTTP Handler] --queue--> [mpsc channel] --dequeue--> [Worker]
     |                                                    |
     |                                                    v
     |<--------------[oneshot channel]<------------- Process & Respond
```

The handler sends the request to a channel (mpsc = multi-producer, single-consumer). The worker picks it up, processes it (checking cache, calling Ollama if needed), and sends the response back through a oneshot channel.

### Health Checks

Every 30 seconds (configurable), the gateway pings each backend at `/api/tags`. If a backend does not respond or returns an error, it is marked as unhealthy.

Unhealthy backends are automatically:
- Skipped when routing requests
- Re-checked periodically
- Marked healthy again when they recover

## Monitoring with Prometheus and Grafana

### Prometheus Setup

Add to your `prometheus.yml`:
```yaml
scrape_configs:
  - job_name: 'ollama-gateway'
    static_configs:
      - targets: ['localhost:8080']
```

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `ollama_requests_total` | Counter | Total requests received |
| `ollama_cache_hits_total` | Counter | Requests served from cache |
| `ollama_cache_misses_total` | Counter | Requests forwarded to Ollama |
| `ollama_cache_size` | Gauge | Current number of cached items |
| `ollama_request_latency_seconds` | Histogram | Request processing time |

### Useful Queries

Cache hit rate:
```
ollama_cache_hits_total / ollama_requests_total * 100
```

Requests per second:
```
rate(ollama_requests_total[5m])
```

Average latency:
```
rate(ollama_request_latency_seconds_sum[5m]) / rate(ollama_request_latency_seconds_count[5m])
```

## Project Structure

```
ollama-gateway/
├── src/
│   ├── main.rs           # Entry point, server setup
│   ├── config.rs         # CLI argument parsing
│   ├── models.rs         # Request/Response data structures
│   ├── metrics.rs        # Prometheus metrics definitions
│   ├── cache.rs          # Caching logic
│   ├── rate_limit.rs     # Rate limiting
│   ├── state.rs          # Shared application state
│   ├── worker.rs         # Background request processor
│   ├── load_balancer.rs  # Load balancing and health checks
│   └── handlers/
│       ├── mod.rs        # Handler exports
│       ├── health.rs     # /health endpoint
│       ├── metrics.rs    # /metrics endpoint
│       └── generate.rs   # /api/generate endpoint
├── Cargo.toml            # Dependencies
└── README.md             # This file
```

## How It Works (Architecture)

```
                    +-------------------+
                    |    HTTP Request   |
                    +--------+----------+
                             |
                             v
                    +--------+----------+
                    |   Rate Limiter    |
                    |  (check limits)   |
                    +--------+----------+
                             |
                             v
                    +--------+----------+
                    |   Request Queue   |
                    |  (mpsc channel)   |
                    +--------+----------+
                             |
                             v
                    +--------+----------+
                    |  Background Worker|
                    |                   |
                    |  1. Check Cache   |
                    |  2. If miss:      |
                    |     - Pick backend|
                    |     - Call Ollama |
                    |     - Cache result|
                    |  3. Send response |
                    +--------+----------+
                             |
            +----------------+----------------+
            |                                 |
            v                                 v
    +-------+-------+                +--------+-------+
    |   Ollama 1    |                |   Ollama 2     |
    | (primary)     |                | (secondary)    |
    +---------------+                +----------------+
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| axum | HTTP server framework |
| tokio | Async runtime |
| reqwest | HTTP client for calling Ollama |
| dashmap | Thread-safe concurrent HashMap |
| prometheus | Metrics collection |
| clap | Command line argument parsing |
| serde | JSON serialization |
| sha2 | Hashing for cache keys |
| chrono | Timestamps |

## Use Cases

**Development/Testing**: Running the same prompts repeatedly while building an app? Cache prevents wasting time and GPU cycles.

**Multi-user Environment**: Multiple developers sharing one Ollama server? Rate limiting prevents one person from hogging resources.

**High Availability**: Production app that cannot afford downtime? Load balancing with health checks provides automatic failover.

**Cost Optimization**: Running Ollama on expensive cloud GPU? Caching reduces inference calls, saving money.

**Monitoring**: Need visibility into LLM usage? Prometheus metrics show exactly what is happening.

## Limitations

- Streaming responses (`stream: true`) not supported yet
- Cache is in-memory only (lost on restart)
- No authentication or API keys
- Exact-match caching only (not semantic similarity)

## Troubleshooting

### Gateway starts but requests fail

Check if Ollama is running:
```bash
curl http://localhost:11434/api/tags
```

### Rate limit kicks in too fast

Increase the limit:
```bash
cargo run -- --rate-limit 100 --rate-window 60
```

### Cache not working

Make sure you are sending the exact same prompt. Even a small difference (extra space, different capitalization) creates a different cache key.

### Backend marked unhealthy

Check if Ollama is accessible from the gateway:
```bash
curl http://your-ollama-host:11434/api/tags
```

## License

MIT License

## Author

Built by [YEDASAVG](https://github.com/YEDASAVG) as a learning project to explore Rust systems programming with async/await, channels, and concurrent data structures.

## Contributing

Contributions are welcome. Please open an issue first to discuss what you would like to change.
