import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSettings,
  DictationResult,
  DownloadProgress,
  OllamaStatus,
  PipelineEvent,
  SttModelStatus,
} from "./types";

function formatBytes(n: number) {
  return `${(n / 1024 / 1024).toFixed(0)} MB`;
}

function StatusDot({ ok, pending }: { ok: boolean; pending?: boolean }) {
  const color = pending ? "bg-amber-400" : ok ? "bg-emerald-500" : "bg-red-500";
  return <span className={`inline-block h-2.5 w-2.5 rounded-full ${color}`} />;
}

export default function App() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [sttStatus, setSttStatus] = useState<SttModelStatus | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const [pipeline, setPipeline] = useState<PipelineEvent>({ state: "idle", detail: null });
  const [lastResult, setLastResult] = useState<DictationResult | null>(null);
  const [dictionaryText, setDictionaryText] = useState("");

  const refreshStatus = useCallback(async () => {
    try {
      const [stt, ol, acc] = await Promise.all([
        invoke<SttModelStatus>("stt_model_status"),
        invoke<OllamaStatus>("ollama_status"),
        invoke<boolean>("check_accessibility"),
      ]);
      setSttStatus(stt);
      setOllama(ol);
      setAccessibility(acc);
    } catch (e) {
      console.error(e);
    }
  }, []);

  useEffect(() => {
    invoke<AppSettings>("get_settings").then((s) => {
      setSettings(s);
      setDictionaryText(s.dictionary.join(", "));
    });
    refreshStatus();
    const poll = setInterval(refreshStatus, 5000);

    const unlistens = [
      listen<DownloadProgress>("stt-model-progress", (e) => {
        setProgress(e.payload);
        if (e.payload.stage === "ready") refreshStatus();
      }),
      listen<PipelineEvent>("pipeline-state", (e) => setPipeline(e.payload)),
      listen<DictationResult>("dictation-complete", (e) => setLastResult(e.payload)),
    ];
    return () => {
      clearInterval(poll);
      unlistens.forEach((u) => u.then((fn) => fn()));
    };
  }, [refreshStatus]);

  const update = (patch: Partial<AppSettings>) => {
    if (!settings) return;
    setSettings({ ...settings, ...patch });
    setDirty(true);
  };

  const save = async () => {
    if (!settings) return;
    const dictionary = dictionaryText
      .split(/[,\n]/)
      .map((w) => w.trim())
      .filter(Boolean);
    const next = { ...settings, dictionary };
    try {
      await invoke("set_settings", { settings: next });
      setSettings(next);
      setDirty(false);
      setSaveError(null);
    } catch (e) {
      setSaveError(String(e));
    }
  };

  if (!settings) return null;

  const modelReady = sttStatus?.loaded ?? false;
  const downloading =
    progress && ["downloading", "verifying", "extracting", "loading"].includes(progress.stage);

  return (
    <div className="h-full overflow-y-auto bg-neutral-950 text-neutral-100">
      <div className="mx-auto max-w-2xl space-y-6 p-6">
        <header className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold">Flow Local</h1>
            <p className="text-sm text-neutral-400">
              Hold <kbd className="rounded bg-neutral-800 px-1.5 py-0.5 text-xs">{settings.hotkey}</kbd>{" "}
              anywhere, speak, release.
            </p>
          </div>
          <span className="rounded-full bg-neutral-800 px-3 py-1 text-xs capitalize text-neutral-300">
            {pipeline.state}
          </span>
        </header>

        {/* Status */}
        <section className="space-y-3 rounded-xl bg-neutral-900 p-4">
          <h2 className="text-sm font-semibold text-neutral-300">Status</h2>
          <div className="space-y-2 text-sm">
            <div className="flex items-center gap-2">
              <StatusDot ok={modelReady} pending={!!downloading} />
              <span>
                Speech model ({sttStatus?.model_name ?? "Parakeet V3"}):{" "}
                {modelReady
                  ? "ready"
                  : downloading
                    ? progress!.stage === "downloading"
                      ? `downloading ${formatBytes(progress!.downloaded_bytes)} / ${formatBytes(progress!.total_bytes)}`
                      : progress!.stage
                    : progress?.stage === "error"
                      ? `error: ${progress.error}`
                      : "not loaded"}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <StatusDot ok={!!ollama?.reachable && !!ollama?.configured_model_available} />
              <span>
                Ollama:{" "}
                {!ollama?.reachable
                  ? "not running — start with `ollama serve`"
                  : ollama.configured_model_available
                    ? `ready (${settings.ollama_model})`
                    : `running, but ${settings.ollama_model} not pulled — run \`ollama pull ${settings.ollama_model}\``}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <StatusDot ok={accessibility === true} />
              <span>Accessibility permission: {accessibility ? "granted" : "needed to insert text"}</span>
              {!accessibility && (
                <button
                  className="rounded bg-sky-600 px-2 py-0.5 text-xs hover:bg-sky-500"
                  onClick={() => invoke("request_accessibility").then(() => refreshStatus())}
                >
                  Grant
                </button>
              )}
            </div>
          </div>
        </section>

        {/* Settings */}
        <section className="space-y-4 rounded-xl bg-neutral-900 p-4">
          <h2 className="text-sm font-semibold text-neutral-300">Settings</h2>

          <div className="grid grid-cols-2 gap-4">
            <label className="space-y-1 text-sm">
              <span className="text-neutral-400">Hotkey</span>
              <input
                className="w-full rounded bg-neutral-800 px-2 py-1.5"
                value={settings.hotkey}
                onChange={(e) => update({ hotkey: e.target.value })}
                placeholder="alt+space"
              />
            </label>
            <label className="space-y-1 text-sm">
              <span className="text-neutral-400">Activation</span>
              <select
                className="w-full rounded bg-neutral-800 px-2 py-1.5"
                value={settings.activation_mode}
                onChange={(e) =>
                  update({ activation_mode: e.target.value as AppSettings["activation_mode"] })
                }
              >
                <option value="push_to_talk">Hold to talk</option>
                <option value="toggle">Press to toggle</option>
              </select>
            </label>
          </div>

          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={settings.cleanup_enabled}
              onChange={(e) => update({ cleanup_enabled: e.target.checked })}
            />
            <span>Clean up transcript with Ollama (falls back to raw text if unavailable)</span>
          </label>

          <div className="grid grid-cols-2 gap-4">
            <label className="space-y-1 text-sm">
              <span className="text-neutral-400">Ollama URL</span>
              <input
                className="w-full rounded bg-neutral-800 px-2 py-1.5"
                value={settings.ollama_url}
                onChange={(e) => update({ ollama_url: e.target.value })}
              />
            </label>
            <label className="space-y-1 text-sm">
              <span className="text-neutral-400">Cleanup model</span>
              {ollama && ollama.models.length > 0 ? (
                <select
                  className="w-full rounded bg-neutral-800 px-2 py-1.5"
                  value={settings.ollama_model}
                  onChange={(e) => update({ ollama_model: e.target.value })}
                >
                  {!ollama.models.includes(settings.ollama_model) && (
                    <option value={settings.ollama_model}>{settings.ollama_model} (not pulled)</option>
                  )}
                  {ollama.models.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  className="w-full rounded bg-neutral-800 px-2 py-1.5"
                  value={settings.ollama_model}
                  onChange={(e) => update({ ollama_model: e.target.value })}
                />
              )}
            </label>
          </div>

          <label className="block space-y-1 text-sm">
            <span className="text-neutral-400">Personal dictionary (names, jargon — comma separated)</span>
            <textarea
              className="h-20 w-full rounded bg-neutral-800 px-2 py-1.5"
              value={dictionaryText}
              onChange={(e) => {
                setDictionaryText(e.target.value);
                setDirty(true);
              }}
              placeholder="Talkdesk, Kubernetes, Lawrence, qwen"
            />
          </label>

          <div className="flex items-center gap-3">
            <button
              className="rounded bg-sky-600 px-4 py-1.5 text-sm font-medium hover:bg-sky-500 disabled:opacity-40"
              disabled={!dirty}
              onClick={save}
            >
              Save
            </button>
            {saveError && <span className="text-xs text-red-400">{saveError}</span>}
          </div>
        </section>

        {/* Last dictation */}
        {lastResult && (
          <section className="space-y-2 rounded-xl bg-neutral-900 p-4">
            <h2 className="text-sm font-semibold text-neutral-300">Last dictation</h2>
            <p className="text-xs text-neutral-500">
              {lastResult.app_name ? `→ ${lastResult.app_name} · ` : ""}
              {lastResult.audio_seconds.toFixed(1)}s audio · STT {lastResult.stt_ms}ms
              {lastResult.llm_ms != null ? ` · cleanup ${lastResult.llm_ms}ms` : " · raw (no cleanup)"}
            </p>
            <p className="whitespace-pre-wrap rounded bg-neutral-800 p-2 text-sm">{lastResult.final_text}</p>
            {lastResult.cleaned && lastResult.raw_text !== lastResult.final_text && (
              <details className="text-xs text-neutral-400">
                <summary className="cursor-pointer">Raw transcript</summary>
                <p className="mt-1 whitespace-pre-wrap">{lastResult.raw_text}</p>
              </details>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
