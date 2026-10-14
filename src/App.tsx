import { useCallback, useEffect, useRef, useState } from "react";
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
  microphone: string | null;
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

/** Live dictation state: what pressing the hotkey would do right now. */
function StatusChip({ pipeline, modelReady }: { pipeline: PipelineEvent; modelReady: boolean }) {
  const LABELS: Record<string, string> = {
    recording: "Listening…",
    transcribing: "Transcribing…",
    cleaning: "Polishing…",
    pasting: "Inserting…",
  };
  let dot: string;
  let label: string;
  let tone: string;
  if (pipeline.state === "error") {
    dot = "bg-red-500";
    label = pipeline.detail ?? "Error";
    tone = "text-red-600";
  } else if (pipeline.state !== "idle") {
    dot = "animate-pulse bg-(--color-accent)";
    label = LABELS[pipeline.state] ?? pipeline.state;
    tone = "text-(--color-accent)";
  } else if (!modelReady) {
    dot = "animate-pulse bg-amber-400";
    label = "Loading speech model…";
    tone = "text-(--color-ink-soft)";
  } else {
    dot = "bg-(--color-accent)";
    label = "Ready to dictate";
    tone = "text-(--color-ink-soft)";
  }
  return (
    <div
      className={`mb-3 flex items-center gap-2 rounded-lg border border-(--color-line) bg-(--color-card) px-3 py-2 text-xs ${tone}`}
      title="Live dictation status — mirrors the on-screen pill while you dictate"
    >
      <span className={`h-2 w-2 shrink-0 rounded-full ${dot}`} />
      <span className="truncate">{label}</span>
    </div>
  );
}

export default function App() {
  const [page, setPage] = useState<Page>("home");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [sttStatus, setSttStatus] = useState<SttModelStatus | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const [microphone, setMicrophone] = useState<string | null>(null);
  const [pipeline, setPipeline] = useState<PipelineEvent>({ state: "idle", detail: null });
  const [lastResult, setLastResult] = useState<DictationResult | null>(null);
  const [toast, setToast] = useState<Achievement | null>(null);

  const refreshPermissions = useCallback(async () => {
    try {
      const [acc, mic] = await Promise.all([
        invoke<boolean>("check_accessibility"),
        invoke<string>("check_microphone"),
      ]);
      setAccessibility(acc);
      setMicrophone(mic);
    } catch (e) {
      console.error(e);
    }
  }, []);

  const prevOllamaReachable = useRef<boolean | null>(null);
  const refreshOllama = useCallback(async () => {
    try {
      const ol = await invoke<OllamaStatus>("ollama_status");
      setOllama(ol);
      // Re-warm model if Ollama just came back online (restarted after sleep etc.)
      if (ol.reachable && ol.configured_model_available && prevOllamaReachable.current === false) {
        invoke("warm_ollama").catch(() => {});
      }
      prevOllamaReachable.current = ol.reachable;
    } catch (e) {
      console.error(e);
    }
  }, []);

  // Called once on mount and when the STT model finishes loading.
  const refreshStatus = useCallback(async () => {
    try {
      const [stt, ol, acc, mic] = await Promise.all([
        invoke<SttModelStatus>("stt_model_status"),
        invoke<OllamaStatus>("ollama_status"),
        invoke<boolean>("check_accessibility"),
        invoke<string>("check_microphone"),
      ]);
      setSttStatus(stt);
      setOllama(ol);
      setAccessibility(acc);
      setMicrophone(mic);
    } catch (e) {
      console.error(e);
    }
  }, []);

  useEffect(() => {
    invoke<AppSettings>("get_settings").then(setSettings);
    refreshStatus();

    // Permissions only change when the user visits System Settings;
    // re-check when the window comes back into focus.
    const onFocus = () => refreshPermissions();
    window.addEventListener("focus", onFocus);

    // Ollama can start/stop any time — poll at a relaxed cadence.
    const ollamaPoll = setInterval(refreshOllama, 15_000);

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
      window.removeEventListener("focus", onFocus);
      clearInterval(ollamaPoll);
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
    microphone,
    pipeline,
    lastResult,
    refreshStatus,
  };

  return (
    <div className="shell flex h-full overflow-hidden text-[15px] text-(--color-ink)">
      {/* Sidebar */}
      <aside className="relative z-10 flex w-56 shrink-0 flex-col border-r border-(--color-line) bg-(--color-card)/70 backdrop-blur">
        <div className="flex items-center gap-2.5 px-5 pt-6 pb-7">
          {/* Same artwork as the app icon (src-tauri/icons/icon.png). */}
          <svg viewBox="0 0 1024 1024" className="h-8 w-8" role="img" aria-label="Flow Local">
            <rect width="1024" height="1024" rx="230" fill="#1B1F3A" />
            <rect x="362" y="180" width="300" height="460" rx="150" fill="none" stroke="#fff" strokeWidth="44" />
            <g fill="#5EE0C8">
              <rect x="418" y="350" width="28" height="120" rx="14" />
              <rect x="458" y="300" width="28" height="220" rx="14" />
              <rect x="498" y="260" width="28" height="300" rx="14" />
              <rect x="538" y="320" width="28" height="180" rx="14" />
              <rect x="578" y="365" width="28" height="90" rx="14" />
            </g>
            <g fill="none" stroke="#fff" strokeWidth="44" strokeLinecap="round">
              <path d="M262 560A250 250 0 0 0 762 560" />
              <path d="M512 810v70M400 880h224" />
            </g>
          </svg>
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
          <StatusChip pipeline={pipeline} modelReady={sttStatus?.loaded ?? false} />

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
