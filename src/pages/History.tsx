import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { HistoryEntry } from "../types";

export default function History() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [search, setSearch] = useState("");

  const refresh = useCallback(async (query: string) => {
    setEntries(await invoke<HistoryEntry[]>("get_history", { search: query || null, limit: 200 }));
  }, []);

  useEffect(() => {
    refresh(search);
  }, [search, refresh]);

  useEffect(() => {
    const unlisten = listen("dictation-complete", () => refresh(search));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [search, refresh]);

  return (
    <div className="space-y-5">
      <header className="rise">
        <h1 className="font-display text-[28px] font-semibold tracking-tight">History</h1>
        <p className="mt-1 text-sm text-(--color-ink-soft)">
          All transcripts are private and stored locally on your Mac.
        </p>
      </header>

      <div className="rise rise-1 flex items-center gap-2">
        <input
          className="flex-1 rounded-lg border border-(--color-line) bg-(--color-card) px-3 py-2 text-sm outline-none placeholder:text-(--color-ink-soft)/60 focus:border-(--color-accent)"
          placeholder="Search dictations…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        {entries.length > 0 && (
          <button
            className="rounded-lg border border-(--color-line) bg-(--color-card) px-3 py-2 text-xs text-red-600 hover:bg-red-50"
            onClick={async () => {
              await invoke("clear_history");
              refresh(search);
            }}
          >
            Clear all
          </button>
        )}
      </div>

      {entries.length === 0 && (
        <p className="rise rise-2 py-10 text-center text-sm text-(--color-ink-soft)">
          No dictations yet. Hold the hotkey and say hello.
        </p>
      )}

      <ul className="rise rise-2 space-y-2">
        {entries.map((e) => (
          <li
            key={e.id}
            className="group rounded-xl border border-(--color-line) bg-(--color-card) p-3.5 shadow-[0_1px_2px_rgb(0_0_0/0.03)]"
          >
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[11px] text-(--color-ink-soft)">
                {new Date(e.ts).toLocaleString(undefined, {
                  month: "short",
                  day: "numeric",
                  hour: "numeric",
                  minute: "2-digit",
                })}
                {e.app_name ? ` · ${e.app_name}` : ""} · {e.words} words · {e.stt_ms}ms
                {e.llm_ms != null ? ` + ${e.llm_ms}ms` : ""}
              </span>
              <span className="flex gap-3 opacity-0 transition group-hover:opacity-100">
                <button
                  className="text-xs text-(--color-accent) hover:underline"
                  onClick={() => navigator.clipboard.writeText(e.final_text)}
                >
                  Copy
                </button>
                <button
                  className="text-xs text-red-600 hover:underline"
                  onClick={async () => {
                    await invoke("delete_history_entry", { id: e.id });
                    refresh(search);
                  }}
                >
                  Delete
                </button>
              </span>
            </div>
            <p className="mt-1.5 text-sm leading-relaxed whitespace-pre-wrap">{e.final_text}</p>
            {e.cleaned && e.raw_text !== e.final_text && (
              <details className="mt-1.5 text-xs text-(--color-ink-soft)">
                <summary className="cursor-pointer select-none">Raw transcript</summary>
                <p className="mt-1 whitespace-pre-wrap">{e.raw_text}</p>
              </details>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
