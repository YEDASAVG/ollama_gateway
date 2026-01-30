# Ollama Gateway

High-performance caching proxy for Ollama with TTL, Prometheus metrics, and request batching.

## Features

- **Response Caching** - Cache LLM responses for instant retrieval
- **TTL Support** - Automatic cache expiration (configurable)
- **Prometheus Metrics** - Full observability with `/metrics` endpoint
- **Proxy to Ollama** - Transparent forwarding to Ollama API

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.70+)
- [Ollama](https://ollama.ai/) running locally

### Installation

```bash
git clone https://github.com/YOUR_USERNAME/ollama-gateway.git
cd ollama-gateway
cargo build --release
```

### Run

```bash
# Make sure Ollama is running on localhost:11434
cargo run
```

Server starts at `http://localhost:8080`

## Usage

### Generate Text

```bash
curl -X POST http://localhost:8080/api/generate \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.2:1b", "prompt": "What is 2+2?", "stream": false}'
```

### View Metrics

```bash
curl http://localhost:8080/metrics
```

## Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `ollama_requests_total` | Counter | Total number of requests |
| `ollama_cache_hits_total` | Counter | Total cache hits |
| `ollama_cache_misses_total` | Counter | Total cache misses |
| `ollama_cache_size` | Gauge | Current cache size |
| `ollama_request_latency_seconds` | Histogram | Request latency distribution |

## Performance

| Scenario | Response Time |
|----------|---------------|
| Without cache (Ollama call) | ~10-20s |
| With cache (HIT) | ~0.009s |
| **Speedup** | **~2000x faster** |

## Architecture

```
┌─────────────┐      ┌─────────────────┐      ┌─────────────┐
│   Client    │ ───▶ │  Ollama Gateway │ ───▶ │   Ollama    │
└─────────────┘      │                 │      └─────────────┘
                     │  • Caching      │
                     │  • TTL          │
                     │  • Metrics      │
                     └─────────────────┘
```

## Configuration

| Constant | Default | Description |
|----------|---------|-------------|
| `OLLAMA_URL` | `http://localhost:11434` | Ollama server URL |
| `CACHE_TTL_SECONDS` | `3600` | Cache expiration time (1 hour) |

## Roadmap

- [x] Basic HTTP proxy
- [x] Response caching
- [x] Cache TTL (expiration)
- [x] Prometheus metrics
- [ ] Grafana dashboard
- [ ] Request batching
- [ ] Load balancing (multiple Ollama backends)
- [ ] CLI arguments (`--port`, `--cache-ttl`)
- [ ] Streaming support

## Tech Stack

- **Language:** Rust
- **HTTP Server:** Axum
- **Async Runtime:** Tokio
- **Caching:** DashMap (thread-safe)
- **Metrics:** Prometheus

## License

MIT

---

Made for faster local LLM inference
