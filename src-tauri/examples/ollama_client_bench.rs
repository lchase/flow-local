//! Throwaway benchmark for wayfinder ticket 04 (Ollama cleanup speed).
//! Measures reqwest::Client::new() per-call vs a shared client, and confirms
//! prefix-KV-cache reuse holds once the system prompt carries real
//! dictionary + app-profile content (not just the bare BASE_PROMPT).
//! cleanup.rs's modules are private to the crate, so this mirrors
//! build_system_prompt's real output shape rather than calling it directly.
//! Run: cargo run --example ollama_client_bench --release

use std::time::Instant;

const URL: &str = "http://localhost:11434/api/tags";
const CHAT_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "qwen3:4b-instruct";

// Mirrors build_system_prompt's output shape with a non-empty dictionary and
// a "code" mode fragment, so the cache check isn't just re-proving the
// bare-BASE_PROMPT case the baseline ticket already measured.
const SYSTEM_PROMPT: &str = "You are a dictation post-processor. The user dictated text by voice; \
you receive the raw speech-to-text transcript. Rewrite it as clean written text:\n\
- Remove filler words (um, uh, like, you know) and false starts.\n\
- Fix punctuation, capitalization, and obvious transcription errors.\n\
- Apply the speaker's self-corrections: \"at 2 actually 3\" becomes \"at 3\".\n\
- Honor spoken commands: \"new paragraph\" -> paragraph break, \"new line\" -> line break, \
\"quote ... end quote\" -> quotation marks.\n\
- Preserve the speaker's wording, meaning, and tone. Never add new content, \
never answer questions in the transcript, never comment on it.\n\
Output ONLY the cleaned text, nothing else.\n\n\
The speaker often uses these words and names; spell them exactly like this when they appear: \
Kubernetes, Postgres, Wispr Flow, Ollama.\n\n\
Context: the text is being inserted into a CODE EDITOR or TERMINAL.\n\
- Preserve technical terms, package names, commands, paths, and flags exactly.";

#[tokio::main]
async fn main() {
    // Part 1: fresh Client::new() per call (today's pre-fix behavior on `main`).
    println!("-- fresh Client::new() per call (/api/tags, cheap no-inference endpoint) --");
    for i in 1..=5 {
        let started = Instant::now();
        let client = reqwest::Client::new();
        let _ = client.get(URL).send().await.expect("request failed");
        println!("run {i}: {:?}", started.elapsed());
    }

    // Part 2: one shared client (the fix).
    println!("-- shared reqwest::Client (fixed) --");
    let shared = reqwest::Client::new();
    for i in 1..=5 {
        let started = Instant::now();
        let _ = shared.get(URL).send().await.expect("request failed");
        println!("run {i}: {:?}", started.elapsed());
    }

    // Part 3: confirm prefix-KV-cache reuse with a real (non-empty dictionary +
    // app-profile) system prompt, using the shared client.
    println!("-- /api/chat prefix-cache check, dictionary+mode-fragment prompt --");
    let transcript = "Let's meet at two, actually three PM. Thanks so much for setting this up.";
    for i in 1..=3 {
        let body = serde_json::json!({
            "model": MODEL,
            "stream": false,
            "keep_alive": -1,
            "options": { "temperature": 0.2 },
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": transcript },
            ],
        });
        let started = Instant::now();
        let resp: serde_json::Value = shared
            .post(CHAT_URL)
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .expect("chat request failed")
            .json()
            .await
            .expect("bad json");
        println!(
            "call {i}: wall={:?} prompt_eval_duration={:?}ns eval_duration={:?}ns",
            started.elapsed(),
            resp.get("prompt_eval_duration"),
            resp.get("eval_duration")
        );
    }
}
