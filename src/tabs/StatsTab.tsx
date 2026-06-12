import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Achievement, Stats } from "../types";

function Big({ value, label }: { value: string; label: string }) {
  return (
    <div className="rounded-lg bg-neutral-900 p-4 text-center">
      <div className="text-2xl font-semibold">{value}</div>
      <div className="mt-1 text-xs text-neutral-500">{label}</div>
    </div>
  );
}

export default function StatsTab() {
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

  if (!stats) return null;

  const maxWords = Math.max(1, ...stats.per_app.map((a) => a.words));

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-3 gap-3">
        <Big value={stats.total_words.toLocaleString()} label="words dictated" />
        <Big value={`${Math.round(stats.avg_wpm)} wpm`} label="average speaking rate" />
        <Big
          value={
            stats.minutes_saved >= 60
              ? `${(stats.minutes_saved / 60).toFixed(1)} h`
              : `${Math.round(stats.minutes_saved)} min`
          }
          label="saved vs typing (45 wpm)"
        />
        <Big value={String(stats.total_dictations)} label="dictations" />
        <Big value={`${stats.streak_days} day${stats.streak_days === 1 ? "" : "s"}`} label="current streak" />
        <Big value={String(stats.distinct_apps)} label="apps used" />
      </div>

      {stats.per_app.length > 0 && (
        <section className="space-y-2 rounded-xl bg-neutral-900 p-4">
          <h2 className="text-sm font-semibold text-neutral-300">By app</h2>
          {stats.per_app.map((a) => (
            <div key={a.app_name} className="flex items-center gap-2 text-sm">
              <span className="w-32 truncate text-neutral-400">{a.app_name}</span>
              <div className="h-3 flex-1 overflow-hidden rounded bg-neutral-800">
                <div
                  className="h-full rounded bg-sky-600"
                  style={{ width: `${(a.words / maxWords) * 100}%` }}
                />
              </div>
              <span className="w-24 text-right text-xs text-neutral-500">
                {a.words.toLocaleString()} words
              </span>
            </div>
          ))}
        </section>
      )}

      <section className="space-y-2 rounded-xl bg-neutral-900 p-4">
        <h2 className="text-sm font-semibold text-neutral-300">
          Achievements ({achievements.filter((a) => a.unlocked_at).length}/{achievements.length})
        </h2>
        <div className="grid grid-cols-2 gap-2">
          {achievements.map((a) => (
            <div
              key={a.id}
              className={`rounded-lg p-3 ${
                a.unlocked_at ? "bg-amber-950/60 text-amber-100" : "bg-neutral-800/50 text-neutral-500"
              }`}
            >
              <div className="text-sm font-medium">
                {a.unlocked_at ? "🏆" : "🔒"} {a.title}
              </div>
              <div className="mt-0.5 text-xs opacity-80">{a.description}</div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
