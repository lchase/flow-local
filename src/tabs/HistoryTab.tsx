import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { HistoryEntry } from "../types";

export default function HistoryTab() {
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
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <input
          className="flex-1 rounded bg-neutral-800 px-3 py-1.5 text-sm"
          placeholder="Search dictations…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        {entries.length > 0 && (
          <button
            className="rounded bg-neutral-800 px-3 py-1.5 text-xs text-red-300 hover:bg-neutral-700"
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
        <p className="py-8 text-center text-sm text-neutral-500">No dictations yet.</p>
      )}

      <ul className="space-y-2">
        {entries.map((e) => (
          <li key={e.id} className="group rounded-lg bg-neutral-900 p-3">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-xs text-neutral-500">
                {new Date(e.ts).toLocaleString()} {e.app_name ? `· ${e.app_name}` : ""} · {e.words}{" "}
                words · STT {e.stt_ms}ms{e.llm_ms != null ? ` · LLM ${e.llm_ms}ms` : ""}
              </span>
              <span className="flex gap-2 opacity-0 transition group-hover:opacity-100">
                <button
                  className="text-xs text-sky-400 hover:text-sky-300"
                  onClick={() => navigator.clipboard.writeText(e.final_text)}
                >
                  Copy
                </button>
                <button
                  className="text-xs text-red-400 hover:text-red-300"
                  onClick={async () => {
                    await invoke("delete_history_entry", { id: e.id });
                    refresh(search);
                  }}
                >
                  Delete
                </button>
              </span>
            </div>
            <p className="mt-1 whitespace-pre-wrap text-sm">{e.final_text}</p>
            {e.cleaned && e.raw_text !== e.final_text && (
              <details className="mt-1 text-xs text-neutral-500">
                <summary className="cursor-pointer">Raw</summary>
                <p className="mt-1 whitespace-pre-wrap">{e.raw_text}</p>
              </details>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
