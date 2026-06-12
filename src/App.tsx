import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Achievement,
  AppProfile,
  AppSettings,
  DictationResult,
  DownloadProgress,
  OllamaStatus,
  PipelineEvent,
  SttModelStatus,
} from "./types";
import HistoryTab from "./tabs/HistoryTab";
import StatsTab from "./tabs/StatsTab";

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
  const [tab, setTab] = useState<"settings" | "history" | "stats">("settings");
  const [toast, setToast] = useState<Achievement | null>(null);
  const [autostart, setAutostart] = useState(false);

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
    invoke<boolean>("get_autostart").then(setAutostart);
    refreshStatus();
    const poll = setInterval(refreshStatus, 5000);

    const unlistens = [
      listen<DownloadProgress>("stt-model-progress", (e) => {
        setProgress(e.payload);
        if (e.payload.stage === "ready") refreshStatus();
      }),
      listen<PipelineEvent>("pipeline-state", (e) => setPipeline(e.payload)),
      listen<DictationResult>("dictation-complete", (e) => setLastResult(e.payload)),
      listen<Achievement[]>("achievements-unlocked", (e) => {
        if (e.payload.length > 0) {
          setToast(e.payload[0]);
          setTimeout(() => setToast(null), 5000);
        }
      }),
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

        <nav className="flex gap-1 rounded-lg bg-neutral-900 p-1">
          {(["settings", "history", "stats"] as const).map((t) => (
            <button
              key={t}
              className={`flex-1 rounded-md px-3 py-1.5 text-sm capitalize ${
                tab === t ? "bg-neutral-700 text-white" : "text-neutral-400 hover:text-neutral-200"
              }`}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </nav>

        {tab === "history" && <HistoryTab />}
        {tab === "stats" && <StatsTab />}

        {tab === "settings" && (
        <>
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
              <span className="text-neutral-400">Speech model</span>
              <select
                className="w-full rounded bg-neutral-800 px-2 py-1.5"
                value={settings.stt_model_id}
                onChange={async (e) => {
                  const id = e.target.value;
                  setSettings({ ...settings, stt_model_id: id });
                  await invoke("set_stt_model", { modelId: id });
                  refreshStatus();
                }}
              >
                {(sttStatus?.models ?? []).map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name} ({m.size_mb} MB{m.downloaded ? "" : ", will download"})
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-end gap-2 pb-1.5 text-sm">
              <input
                type="checkbox"
                checked={autostart}
                onChange={async (e) => {
                  setAutostart(e.target.checked);
                  await invoke("set_autostart", { enabled: e.target.checked });
                }}
              />
              <span>Launch at login</span>
            </label>
          </div>

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

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-sm text-neutral-400">
                Per-app style (code editors, terminals, Slack and Mail are detected automatically)
              </span>
              <button
                className="rounded bg-neutral-800 px-2 py-0.5 text-xs hover:bg-neutral-700"
                onClick={() =>
                  update({
                    app_profiles: [
                      ...settings.app_profiles,
                      {
                        bundle_id: lastResult?.app_bundle_id ?? "",
                        mode: "prose",
                      },
                    ],
                  })
                }
              >
                + Add override
              </button>
            </div>
            {settings.app_profiles.map((p, i) => (
              <div key={i} className="flex items-center gap-2">
                <input
                  className="flex-1 rounded bg-neutral-800 px-2 py-1 text-sm"
                  value={p.bundle_id}
                  placeholder="bundle id, e.g. com.microsoft.VSCode"
                  onChange={(e) => {
                    const next = [...settings.app_profiles];
                    next[i] = { ...p, bundle_id: e.target.value };
                    update({ app_profiles: next });
                  }}
                />
                <select
                  className="rounded bg-neutral-800 px-2 py-1 text-sm"
                  value={p.mode}
                  onChange={(e) => {
                    const next = [...settings.app_profiles];
                    next[i] = { ...p, mode: e.target.value as AppProfile["mode"] };
                    update({ app_profiles: next });
                  }}
                >
                  <option value="code">Code</option>
                  <option value="messaging">Messaging</option>
                  <option value="email">Email</option>
                  <option value="prose">Prose</option>
                </select>
                <button
                  className="rounded bg-neutral-800 px-2 py-1 text-xs text-red-300 hover:bg-neutral-700"
                  onClick={() =>
                    update({ app_profiles: settings.app_profiles.filter((_, j) => j !== i) })
                  }
                >
                  ✕
                </button>
              </div>
            ))}
          </div>

          <label className="flex items-center gap-2 text-sm">
            <span className="text-neutral-400">Keep history</span>
            <select
              className="rounded bg-neutral-800 px-2 py-1"
              value={settings.history_retention}
              onChange={(e) =>
                update({ history_retention: e.target.value as AppSettings["history_retention"] })
              }
            >
              <option value="forever">Forever</option>
              <option value="24h">24 hours</option>
              <option value="never">Don't store</option>
            </select>
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
        </>
        )}

        {/* Last dictation */}
        {lastResult && tab === "settings" && (
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

      {toast && (
        <div className="fixed bottom-4 right-4 max-w-xs rounded-xl bg-amber-900/95 p-4 text-amber-50 shadow-2xl">
          <div className="text-sm font-semibold">🏆 {toast.title}</div>
          <div className="mt-0.5 text-xs opacity-90">{toast.description}</div>
        </div>
      )}
    </div>
  );
}
