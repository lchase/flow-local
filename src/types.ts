export type ProfileMode = "code" | "messaging" | "email" | "prose";

export interface AppProfile {
  bundle_id: string;
  mode: ProfileMode;
}

export interface AppSettings {
  hotkey: string;
  activation_mode: "push_to_talk" | "toggle";
  stt_model_id: string;
  cleanup_enabled: boolean;
  ollama_url: string;
  ollama_model: string;
  dictionary: string[];
  app_profiles: AppProfile[];
  history_retention: "forever" | "24h" | "never";
}

export interface HistoryEntry {
  id: number;
  ts: string;
  app_name: string | null;
  app_bundle_id: string | null;
  raw_text: string;
  final_text: string;
  cleaned: boolean;
  audio_seconds: number;
  stt_ms: number;
  llm_ms: number | null;
  words: number;
}

export interface AppCount {
  app_name: string;
  dictations: number;
  words: number;
}

export interface Stats {
  total_dictations: number;
  total_words: number;
  total_audio_seconds: number;
  avg_wpm: number;
  minutes_saved: number;
  streak_days: number;
  distinct_apps: number;
  days_active: number;
  per_app: AppCount[];
}

export interface Achievement {
  id: string;
  title: string;
  description: string;
  unlocked_at: string | null;
}

export interface PipelineEvent {
  state: "idle" | "recording" | "transcribing" | "cleaning" | "pasting" | "error";
  detail: string | null;
}

export interface DictationResult {
  raw_text: string;
  final_text: string;
  cleaned: boolean;
  app_name: string | null;
  app_bundle_id: string | null;
  audio_seconds: number;
  stt_ms: number;
  llm_ms: number | null;
}

export interface DownloadProgress {
  model_id: string;
  downloaded_bytes: number;
  total_bytes: number;
  stage: "downloading" | "verifying" | "extracting" | "loading" | "ready" | "error";
  error: string | null;
}

export interface SttModelChoice {
  id: string;
  name: string;
  description: string;
  size_mb: number;
  downloaded: boolean;
}

export interface SttModelStatus {
  model_id: string;
  model_name: string;
  downloaded: boolean;
  loaded: boolean;
  models: SttModelChoice[];
}

export interface OllamaStatus {
  reachable: boolean;
  models: string[];
  configured_model_available: boolean;
}
