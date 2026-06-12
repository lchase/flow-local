export interface AppSettings {
  hotkey: string;
  activation_mode: "push_to_talk" | "toggle";
  cleanup_enabled: boolean;
  ollama_url: string;
  ollama_model: string;
  dictionary: string[];
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

export interface SttModelStatus {
  model_id: string;
  model_name: string;
  downloaded: boolean;
  loaded: boolean;
}

export interface OllamaStatus {
  reachable: boolean;
  models: string[];
  configured_model_available: boolean;
}
