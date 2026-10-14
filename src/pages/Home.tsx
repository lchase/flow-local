import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Shared } from "../App";
import type { Achievement, Stats } from "../types";

function greeting() {
  const h = new Date().getHours();
  if (h < 5) return "Up late";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

function fmtBytes(n: number) {
  return `${(n / 1024 / 1024).toFixed(0)} MB`;
}

function fmtTimeSaved(minutes: number) {
  if (minutes < 1) return "< 1 min";
  if (minutes < 60) return `${Math.round(minutes)} min`;
  const hrs = Math.floor(minutes / 60);
  const mins = Math.round(minutes % 60);
  if (hrs < 24) return mins > 0 ? `${hrs} hr ${mins} min` : `${hrs} hr`;
  const days = Math.floor(hrs / 24);
  const remHrs = hrs % 24;
  return remHrs > 0 ? `${days} day ${remHrs} hr` : `${days} days`;
}

function StatCard({
  title,
  value,
  quip,
  delay,
}: {
  title: string;
  value: string;
  quip: string;
  delay: string;
}) {
  return (
    <div className={`rise ${delay} rounded-xl border border-(--color-line) bg-(--color-card) p-4 shadow-[0_1px_2px_rgb(0_0_0/0.03)]`}>
      <div className="text-xs text-(--color-ink-soft)">{title}</div>
      <div className="font-display mt-1.5 text-2xl font-semibold tracking-tight">{value}</div>
      <div className="mt-1.5 text-xs text-(--color-ink-soft)">{quip}</div>
    </div>
  );
}

function Dot({ ok, pending }: { ok: boolean; pending?: boolean }) {
  return (
    <span
      className={`inline-block h-2 w-2 rounded-full ${
        pending ? "animate-pulse bg-amber-400" : ok ? "bg-(--color-accent)" : "bg-red-500"
      }`}
    />
  );
}

export default function Home({ shared }: { shared: Shared }) {
  const { settings, sttStatus, progress, ollama, accessibility, microphone, lastResult, refreshStatus } = shared;
  const [stats, setStats] = useState<Stats | null>(null);
  const [achievements, setAchievements] = useState<Achievement[]>([]);

  const refresh = async () => {
    setStats(await invoke<Stats>("get_stats"));
    setAchievements(await invoke<Achievement[]>("get_achievements"));
  };

  useEffect(() => {
    refresh();
    const unlisten = listen("dictation-complete", refresh);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const modelReady = sttStatus?.loaded ?? false;
  const downloading =
    progress && ["downloading", "verifying", "extracting", "loading"].includes(progress.stage);
  const ollamaReady = !!ollama?.reachable && !!ollama?.configured_model_available;
  const hotkeyParts = settings.hotkey.split("+");

  const words = stats?.total_words ?? 0;
  const unlocked = achievements.filter((a) => a.unlocked_at);

  return (
    <div className="space-y-7">
      <header className="rise">
        <h1 className="font-display text-[28px] font-semibold tracking-tight">{greeting()}.</h1>
        <p className="mt-2 text-(--color-ink-soft)">
          Hold{" "}
          {hotkeyParts.map((k, i) => (
            <span key={i}>
              <kbd>{k === "alt" ? "⌥" : k === "cmd" || k === "super" ? "⌘" : k}</kbd>
              {i < hotkeyParts.length - 1 ? " " : ""}
            </span>
          ))}{" "}
          and speak into any textbox.
        </p>
      </header>

      {/* Status line — only the things that need attention shout */}
      <div className="rise rise-1 flex flex-wrap gap-x-5 gap-y-2 text-xs text-(--color-ink-soft)">
        {downloading ? (
          <div className="flex flex-col gap-1.5">
            <span className="flex items-center gap-1.5">
              <Dot ok={false} pending />
              {progress!.stage === "downloading"
                ? `Downloading · ${fmtBytes(progress!.downloaded_bytes)} / ${fmtBytes(progress!.total_bytes)}`
                : `Model: ${progress!.stage}…`}
            </span>
            <div className="ml-3.5 h-1 w-40 overflow-hidden rounded-full bg-(--color-line)">
              {progress!.stage === "downloading" && progress!.total_bytes > 0 ? (
                <div
                  className="h-full rounded-full bg-(--color-accent) transition-all duration-200 ease-out"
                  style={{ width: `${Math.min(100, Math.round((progress!.downloaded_bytes / progress!.total_bytes) * 100))}%` }}
                />
              ) : (
                <div className="h-full w-full animate-pulse rounded-full bg-(--color-accent)" />
              )}
            </div>
          </div>
        ) : (
          <span className="flex items-center gap-1.5">
            <Dot ok={modelReady} />
            {modelReady ? sttStatus?.model_name : "Speech model not loaded"}
          </span>
        )}
        <span className="flex items-center gap-1.5">
          <Dot ok={ollamaReady} />
          {!ollama?.reachable
            ? "Ollama not running"
            : ollamaReady
              ? `Ollama · ${settings.ollama_model}`
              : `Pull ${settings.ollama_model} in Ollama`}
        </span>
        <span className="flex items-center gap-1.5">
          <Dot ok={accessibility === true} />
          {accessibility ? (
            "Accessibility granted"
          ) : (
            <button
              className="underline decoration-dotted underline-offset-2 hover:text-(--color-ink)"
              onClick={() => invoke("request_accessibility").then(refreshStatus)}
            >
              Grant Accessibility to insert text
            </button>
          )}
        </span>
        <span className="flex items-center gap-1.5">
          <Dot ok={microphone === "granted"} pending={microphone === "not_determined"} />
          {microphone === "granted"
            ? "Microphone granted"
            : microphone === "denied" || microphone === "restricted"
              ? (
                <button
                  className="underline decoration-dotted underline-offset-2 hover:text-(--color-ink)"
                  onClick={() => invoke("open_system_settings_microphone")}
                >
                  Microphone denied — open System Settings
                </button>
              )
              : "Microphone — will ask on first dictation"}
        </span>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <StatCard
          delay="rise-1"
          title="Current streak"
          value={`${stats?.streak_days ?? 0} day${(stats?.streak_days ?? 0) === 1 ? "" : "s"} ${(stats?.streak_days ?? 0) > 0 ? "⚡" : ""}`}
          quip={(stats?.streak_days ?? 0) > 0 ? "Keep the run alive" : "Dictate today to start one"}
        />
        <StatCard
          delay="rise-2"
          title="Average speed"
          value={`${Math.round(stats?.avg_wpm ?? 0)} WPM ${(stats?.avg_wpm ?? 0) > 100 ? "👑" : ""}`}
          quip={(stats?.avg_wpm ?? 0) > 45 ? "Way faster than typing" : "Speak naturally — it adds up"}
        />
        <StatCard
          delay="rise-3"
          title="Time saved"
          value={(stats?.minutes_saved ?? 0) > 0 ? fmtTimeSaved(stats!.minutes_saved) : "—"}
          quip={words > 0 ? `${words.toLocaleString()} words vs. typing at 45 WPM` : "Your first words await"}
        />
      </div>

      {lastResult && (
        <section className="rise rise-3 rounded-xl border border-(--color-line) bg-(--color-card) p-4">
          <div className="flex items-baseline justify-between">
            <h2 className="font-display text-sm font-semibold">Last dictation</h2>
            <span className="text-[11px] text-(--color-ink-soft)">
              {lastResult.app_name ? `${lastResult.app_name} · ` : ""}
              STT {lastResult.stt_ms}ms
              {lastResult.llm_ms != null ? ` · polish ${lastResult.llm_ms}ms` : ""}
            </span>
          </div>
          <p className="mt-2 text-sm leading-relaxed whitespace-pre-wrap">{lastResult.final_text}</p>
        </section>
      )}

      <section className="rise rise-4 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="font-display text-sm font-semibold">Achievements</h2>
          <span className="text-xs text-(--color-ink-soft)">{unlocked.length} / {achievements.length}</span>
        </div>
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-(--color-line)">
          <div
            className="h-full rounded-full bg-amber-400 transition-all duration-500"
            style={{ width: achievements.length ? `${(unlocked.length / achievements.length) * 100}%` : "0%" }}
          />
        </div>
        <div className="grid grid-cols-2 gap-2">
          {achievements.map((a) => (
            <div
              key={a.id}
              className={`rounded-lg border p-3 ${
                a.unlocked_at
                  ? "border-amber-200 bg-(--color-amber-soft)"
                  : "border-(--color-line) bg-(--color-card) opacity-60"
              }`}
            >
              <div className="text-[13px] font-medium">
                {a.unlocked_at ? "🏆" : "🔒"} {a.title}
              </div>
              <div className="mt-0.5 text-xs text-(--color-ink-soft)">{a.description}</div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
