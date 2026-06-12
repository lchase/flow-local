use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::context::FrontmostApp;
use crate::settings::AppSettings;

const BASE_PROMPT: &str = "You are a dictation post-processor. The user dictated text by voice; \
you receive the raw speech-to-text transcript. Rewrite it as clean written text:\n\
- Remove filler words (um, uh, like, you know) and false starts.\n\
- Fix punctuation, capitalization, and obvious transcription errors.\n\
- Apply the speaker's self-corrections: \"at 2 actually 3\" becomes \"at 3\".\n\
- Honor spoken commands: \"new paragraph\" → paragraph break, \"new line\" → line break, \
\"quote ... end quote\" → quotation marks.\n\
- Preserve the speaker's wording, meaning, and tone. Never add new content, \
never answer questions in the transcript, never comment on it.\n\
Output ONLY the cleaned text, nothing else.";

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
    #[serde(default)]
    eval_count: Option<u64>,
    #[serde(default)]
    eval_duration: Option<u64>,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Clone, Serialize)]
pub struct CleanupResult {
    pub text: String,
    pub llm_ms: u128,
    pub tokens_per_sec: Option<f64>,
}

pub fn build_system_prompt(settings: &AppSettings, app: Option<&FrontmostApp>) -> String {
    let mut prompt = String::from(BASE_PROMPT);
    if !settings.dictionary.is_empty() {
        prompt.push_str(&format!(
            "\n\nThe speaker often uses these words and names; spell them exactly like this \
when they appear: {}.",
            settings.dictionary.join(", ")
        ));
    }
    if let Some(app) = app {
        prompt.push_str(&format!(
            "\n\nThe text will be inserted into the application \"{}\".",
            app.name
        ));
    }
    prompt
}

pub async fn clean_transcript(
    settings: &AppSettings,
    app: Option<&FrontmostApp>,
    raw: &str,
) -> Result<CleanupResult> {
    let system = build_system_prompt(settings, app);
    let body = json!({
        "model": settings.ollama_model,
        "stream": false,
        "keep_alive": -1,
        "options": { "temperature": 0.2 },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": raw },
        ],
    });

    let started = std::time::Instant::now();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/chat", settings.ollama_url.trim_end_matches('/')))
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| anyhow!("Ollama unreachable at {}: {e}", settings.ollama_url))?
        .error_for_status()
        .map_err(|e| anyhow!("Ollama error (model pulled?): {e}"))?;

    let parsed: ChatResponse = resp.json().await?;
    let text = parsed.message.content.trim().to_string();
    if text.is_empty() {
        return Err(anyhow!("Ollama returned empty cleanup"));
    }
    let tokens_per_sec = match (parsed.eval_count, parsed.eval_duration) {
        (Some(n), Some(d)) if d > 0 => Some(n as f64 / (d as f64 / 1e9)),
        _ => None,
    };
    Ok(CleanupResult {
        text,
        llm_ms: started.elapsed().as_millis(),
        tokens_per_sec,
    })
}

/// Preload the model so the first dictation doesn't pay the cold-start cost.
pub async fn warm_model(settings: &AppSettings) {
    let body = json!({ "model": settings.ollama_model, "keep_alive": -1, "messages": [] });
    let client = reqwest::Client::new();
    let _ = client
        .post(format!("{}/api/chat", settings.ollama_url.trim_end_matches('/')))
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await;
}

#[derive(Serialize)]
pub struct OllamaStatus {
    pub reachable: bool,
    pub models: Vec<String>,
    pub configured_model_available: bool,
}

pub async fn ollama_status(settings: &AppSettings) -> OllamaStatus {
    #[derive(Deserialize)]
    struct Tags {
        models: Vec<TagModel>,
    }
    #[derive(Deserialize)]
    struct TagModel {
        name: String,
    }

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", settings.ollama_url.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;
    match resp {
        Ok(r) => match r.json::<Tags>().await {
            Ok(tags) => {
                let models: Vec<String> = tags.models.into_iter().map(|m| m.name).collect();
                let configured = models.iter().any(|m| {
                    m == &settings.ollama_model
                        || m.trim_end_matches(":latest")
                            == settings.ollama_model.trim_end_matches(":latest")
                });
                OllamaStatus {
                    reachable: true,
                    models,
                    configured_model_available: configured,
                }
            }
            Err(_) => OllamaStatus {
                reachable: true,
                models: vec![],
                configured_model_available: false,
            },
        },
        Err(_) => OllamaStatus {
            reachable: false,
            models: vec![],
            configured_model_available: false,
        },
    }
}
