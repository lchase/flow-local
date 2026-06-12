import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Achievement,
  AppSettings,
  DictationResult,
  DownloadProgress,
  OllamaStatus,
  PipelineEvent,
  SttModelStatus,
} from "./types";
import Home from "./pages/Home";
import History from "./pages/History";
import Dictionary from "./pages/Dictionary";
import Settings from "./pages/Settings";

export type Page = "home" | "history" | "dictionary" | "settings";

export interface Shared {
  settings: AppSettings;
  saveSettings: (next: AppSettings) => Promise<void>;
  sttStatus: SttModelStatus | null;
  progress: DownloadProgress | null;
  ollama: OllamaStatus | null;
  accessibility: boolean | null;
  pipeline: PipelineEvent;
  lastResult: DictationResult | null;
  refreshStatus: () => void;
}

const NAV: { id: Page; label: string; icon: React.ReactNode }[] = [
  {
    id: "home",
    label: "Home",
    icon: (
      <svg viewBox="0 0 16 16" className="h-4 w-4" fill="currentColor">
        <path d="M2 2h5v5H2V2zm7 0h5v5H9V2zM2 9h5v5H2V9zm7 0h5v5H9V9z" opacity=".85" />
      </svg>
    ),
  },
  {
    id: "history",
    label: "History",
    icon: (
      <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6">
        <circle cx="8" cy="8" r="6" />
        <path d="M8 4.5V8l2.3 1.6" strokeLinecap="round" />
      </svg>
    ),
  },
  {
    id: "dictionary",
    label: "Dictionary",
    icon: (
      <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6">
        <path d="M4 2.5h8.5v11H4a1.5 1.5 0 0 1-1.5-1.5V4A1.5 1.5 0 0 1 4 2.5z" />
        <path d="M12.5 10.5H4a1.5 1.5 0 0 0 0 3" />
      </svg>
    ),
  },
  {
    id: "settings",
    label: "Settings",
    icon: (
      <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6">
        <circle cx="8" cy="8" r="2.2" />
        <path d="M8 1.8v2M8 12.2v2M1.8 8h2M12.2 8h2M3.6 3.6l1.4 1.4M11 11l1.4 1.4M12.4 3.6 11 5M5 11l-1.4 1.4" strokeLinecap="round" />
      </svg>
    ),
  },
];

export default function App() {
  const [page, setPage] = useState<Page>("home");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [sttStatus, setSttStatus] = useState<SttModelStatus | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const [pipeline, setPipeline] = useState<PipelineEvent>({ state: "idle", detail: null });
  const [lastResult, setLastResult] = useState<DictationResult | null>(null);
  const [toast, setToast] = useState<Achievement | null>(null);

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
    invoke<AppSettings>("get_settings").then(setSettings);
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

  const saveSettings = useCallback(async (next: AppSettings) => {
    await invoke("set_settings", { settings: next });
    setSettings(next);
  }, []);

  if (!settings) return null;

  const shared: Shared = {
    settings,
    saveSettings,
    sttStatus,
    progress,
    ollama,
    accessibility,
    pipeline,
    lastResult,
    refreshStatus,
  };

  return (
    <div className="shell flex h-full overflow-hidden text-[15px] text-(--color-ink)">
      {/* Sidebar */}
      <aside className="relative z-10 flex w-56 shrink-0 flex-col border-r border-(--color-line) bg-(--color-card)/70 backdrop-blur">
        <div className="flex items-center gap-2.5 px-5 pt-6 pb-7">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-(--color-ink) text-(--color-paper)">
            <svg viewBox="0 0 16 16" className="h-4 w-4" fill="currentColor">
              <rect x="6" y="1.5" width="4" height="8" rx="2" />
              <path d="M3.5 7.5a4.5 4.5 0 0 0 9 0h-1.3a3.2 3.2 0 0 1-6.4 0H3.5z" />
              <rect x="7.3" y="12" width="1.4" height="2.5" rx="0.7" />
            </svg>
          </div>
          <span className="font-display text-[17px] font-semibold tracking-tight">Flow Local</span>
        </div>

        <nav className="flex flex-col gap-0.5 px-3">
          {NAV.map((item) => (
            <button
              key={item.id}
              onClick={() => setPage(item.id)}
              className={`flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                page === item.id
                  ? "bg-(--color-accent-soft) font-medium text-(--color-accent)"
                  : "text-(--color-ink-soft) hover:bg-black/4 hover:text-(--color-ink)"
              }`}
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </nav>

        <div className="mt-auto px-5 pb-5">
          <div
            className={`mb-3 flex items-center gap-2 rounded-lg border border-(--color-line) bg-(--color-card) px-3 py-2 text-xs ${
              pipeline.state === "idle" ? "text-(--color-ink-soft)" : "text-(--color-accent)"
            }`}
          >
            <span
              className={`h-2 w-2 rounded-full ${
                pipeline.state === "idle"
                  ? "bg-stone-300"
                  : pipeline.state === "error"
                    ? "bg-red-500"
                    : "animate-pulse bg-(--color-accent)"
              }`}
            />
            <span className="capitalize">{pipeline.state === "idle" ? "Ready" : pipeline.state}</span>
          </div>
          <p className="text-[11px] leading-relaxed text-(--color-ink-soft)">
            v0.1.0 · 100% on-device.
            <br />
            Nothing leaves this Mac.
          </p>
        </div>
      </aside>

      {/* Content */}
      <main className="relative z-10 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-2xl px-8 py-8">
          {page === "home" && <Home shared={shared} />}
          {page === "history" && <History />}
          {page === "dictionary" && <Dictionary shared={shared} />}
          {page === "settings" && <Settings shared={shared} />}
        </div>
      </main>

      {toast && (
        <div className="rise fixed right-5 bottom-5 z-20 max-w-xs rounded-xl border border-(--color-line) bg-(--color-card) p-4 shadow-xl">
          <div className="font-display text-sm font-semibold">🏆 {toast.title}</div>
          <div className="mt-0.5 text-xs text-(--color-ink-soft)">{toast.description}</div>
        </div>
      )}
    </div>
  );
}
