use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

// Ollama API request format
#[derive(Deserialize, Serialize, Clone)]
pub struct GenerateRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub stream: bool,
}

// Ollama API response format
#[derive(Deserialize, Serialize, Clone)]
pub struct GenerateResponse {
    pub model: String,
    pub response: String,
}

// Batched request - holds request + response channel
pub struct BatchedRequest {
    pub request: GenerateRequest,  // original request
    pub response_tx: oneshot::Sender<Result<GenerateResponse, String>>, //one-time channel to send back response
}