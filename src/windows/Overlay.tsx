import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PipelineEvent } from "../types";

const LABELS: Record<string, string> = {
  recording: "Listening…",
  transcribing: "Transcribing…",
  cleaning: "Polishing…",
  pasting: "Inserting…",
};

export default function Overlay() {
  const [state, setState] = useState<PipelineEvent>({ state: "idle", detail: null });
  const [level, setLevel] = useState(0);

  useEffect(() => {
    const unlisten = listen<PipelineEvent>("pipeline-state", (e) => setState(e.payload));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (state.state !== "recording") {
      setLevel(0);
      return;
    }
    const timer = setInterval(async () => {
      try {
        setLevel(await invoke<number>("mic_level"));
      } catch {
        /* window closing */
      }
    }, 80);
    return () => clearInterval(timer);
  }, [state.state]);

  const isError = state.state === "error";
  const bars = 5;

  return (
    <div className="flex h-full w-full items-center justify-center">
      <div
        className={`flex h-9 items-center gap-2 rounded-full px-4 text-xs font-medium shadow-lg backdrop-blur ${
          isError ? "bg-red-900/90 text-red-100" : "bg-neutral-900/90 text-neutral-100"
        }`}
      >
        {state.state === "recording" ? (
          <div className="flex items-end gap-0.5">
            {Array.from({ length: bars }).map((_, i) => {
              const intensity = Math.min(1, level * 14 * (1 - Math.abs(i - 2) * 0.18));
              return (
                <div
                  key={i}
                  className="w-1 rounded-full bg-emerald-400 transition-all duration-75"
                  style={{ height: `${5 + intensity * 14}px` }}
                />
              );
            })}
          </div>
        ) : (
          <span
            className={`h-2 w-2 rounded-full ${isError ? "bg-red-400" : "animate-pulse bg-sky-400"}`}
          />
        )}
        <span className="max-w-44 truncate">
          {isError ? (state.detail ?? "Error") : (LABELS[state.state] ?? "")}
        </span>
      </div>
    </div>
  );
}
