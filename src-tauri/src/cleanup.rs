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

/// Built-in bundle-id → mode mapping for common apps; user profiles override.
pub fn builtin_mode(bundle_id: &str) -> Option<&'static str> {
    match bundle_id {
        // Code editors and terminals
        "com.microsoft.VSCode"
        | "com.microsoft.VSCodeInsiders"
        | "com.todesktop.230313mzl4w4u92" // Cursor
        | "com.exafunction.windsurf"
        | "dev.zed.Zed"
        | "com.jetbrains.intellij"
        | "com.googlecode.iterm2"
        | "com.apple.Terminal"
        | "dev.warp.Warp-Stable"
        | "com.github.wez.wezterm"
        | "net.kovidgoyal.kitty"
        | "com.mitchellh.ghostty"
        | "com.apple.dt.Xcode" => Some("code"),
        // Messaging
        "com.tinyspeck.slackmacgap"
        | "com.apple.MobileSMS"
        | "com.hnc.Discord"
        | "net.whatsapp.WhatsApp"
        | "org.telegram.desktop"
        | "ru.keepcoder.Telegram" => Some("messaging"),
        // Email
        "com.apple.mail" | "com.microsoft.Outlook" | "com.superhuman.electron" => Some("email"),
        _ => None,
    }
}

fn mode_fragment(mode: &str) -> &'static str {
    match mode {
        "code" => {
            "\n\nContext: the text is being inserted into a CODE EDITOR or TERMINAL.\n\
- Preserve technical terms, package names, commands, paths, and flags exactly; \
never \"correct\" them to English words.\n\
- When the speaker names a variable, function, or class in prose, render it as \
an identifier: \"user settings object\" → userSettings (camelCase by default; \
use snake_case or kebab-case or SCREAMING_CASE if the speaker asks, e.g. \
\"snake case user id\" → user_id).\n\
- Spoken filenames join up: \"my script dot py\" → my_script.py, \"app dot t s x\" → App.tsx.\n\
- Keep output terse. Don't capitalize the first word or add a trailing period \
to short commands or code fragments."
        }
        "messaging" => {
            "\n\nContext: the text is a CHAT MESSAGE. Keep it casual and conversational. \
Short sentences are fine; no formal greetings or sign-offs; don't add emoji unless spoken."
        }
        "email" => {
            "\n\nContext: the text is an EMAIL. Use a clear, professional, friendly tone \
with proper sentences and paragraphs."
        }
        _ => "",
    }
}

pub fn mode_for_app(settings: &AppSettings, app: Option<&FrontmostApp>) -> String {
    let Some(app) = app else {
        return "prose".to_string();
    };
    if let Some(profile) = settings
        .app_profiles
        .iter()
        .find(|p| p.bundle_id == app.bundle_id)
    {
        return profile.mode.clone();
    }
    builtin_mode(&app.bundle_id).unwrap_or("prose").to_string()
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
    let mode = mode_for_app(settings, app);
    prompt.push_str(mode_fragment(&mode));
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
