import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Shared } from "../App";
import type { AppProfile, AppSettings } from "../types";

const CLEANUP_LEVELS: { value: "none" | "light" | "medium" | "high"; label: string; description: string }[] = [
  { value: "none",   label: "None",   description: "Raw transcript, exactly as heard" },
  { value: "light",  label: "Light",  description: "Remove fillers and fix punctuation" },
  { value: "medium", label: "Medium", description: "Edit for clarity and conciseness" },
  { value: "high",   label: "High",   description: "Rewrite for brevity and polish" },
];

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3">
      <div>
        <div className="text-sm font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-xs text-(--color-ink-soft)">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Section({ title, children, delay }: { title: string; children: React.ReactNode; delay?: string }) {
  return (
    <section className={`rise ${delay ?? ""} rounded-xl border border-(--color-line) bg-(--color-card) px-4 shadow-[0_1px_2px_rgb(0_0_0/0.03)]`}>
      <h2 className="font-display border-b border-(--color-line) py-3 text-sm font-semibold">{title}</h2>
      <div className="divide-y divide-(--color-line)">{children}</div>
    </section>
  );
}

const inputCls =
  "rounded-lg border border-(--color-line) bg-(--color-paper) px-2.5 py-1.5 text-sm outline-none focus:border-(--color-accent)";

const MODIFIER_CODES = new Set(["AltLeft", "AltRight", "MetaLeft", "MetaRight", "ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight"]);

function formatHotkey(binding: string): string {
  return binding
    .split("+")
    .map((tok) => {
      switch (tok.toLowerCase()) {
        case "cmd": case "command": case "super": return "⌘";
        case "alt": case "option": return "⌥";
        case "ctrl": case "control": return "⌃";
        case "shift": return "⇧";
        default: return tok.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Arrow/, "");
      }
    })
    .join("");
}

function HotkeyRecorder({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [listening, setListening] = useState(false);
  const [heldMods, setHeldMods] = useState<string[]>([]);

  useEffect(() => {
    if (!listening) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.code === "Escape") { setListening(false); setHeldMods([]); return; }
      if (MODIFIER_CODES.has(e.code)) {
        const mods: string[] = [];
        if (e.metaKey) mods.push("cmd");
        if (e.altKey) mods.push("alt");
        if (e.ctrlKey) mods.push("ctrl");
        if (e.shiftKey) mods.push("shift");
        setHeldMods(mods);
        return;
      }
      const mods: string[] = [];
      if (e.metaKey) mods.push("cmd");
      if (e.altKey) mods.push("alt");
      if (e.ctrlKey) mods.push("ctrl");
      if (e.shiftKey) mods.push("shift");
      onChange([...mods, e.code].join("+"));
      setListening(false);
      setHeldMods([]);
    };
    // Releasing the sole held modifier (no other key ever pressed) binds it bare —
    // e.g. push-to-talk on Option alone, which a mod+key combo can't express.
    const onKeyUp = (e: KeyboardEvent) => {
      if (!MODIFIER_CODES.has(e.code)) return;
      setHeldMods((mods) => {
        if (mods.length === 1) {
          onChange(mods[0]);
          setListening(false);
          return [];
        }
        const next: string[] = [];
        if (e.metaKey) next.push("cmd");
        if (e.altKey) next.push("alt");
        if (e.ctrlKey) next.push("ctrl");
        if (e.shiftKey) next.push("shift");
        return next;
      });
    };
    const onBlur = () => { setListening(false); setHeldMods([]); };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [listening, onChange]);

  return (
    <button
      type="button"
      className={`${inputCls} w-40 text-left`}
      onClick={() => { setListening(true); setHeldMods([]); }}
      onBlur={() => setListening(false)}
    >
      {listening
        ? (heldMods.length ? formatHotkey(heldMods.join("+")) + "…" : "Press keys…")
        : formatHotkey(value)}
    </button>
  );
}

export default function Settings({ shared }: { shared: Shared }) {
  const { settings, saveSettings, sttStatus, ollama, refreshStatus } = shared;
  const [local, setLocal] = useState<AppSettings>(settings);
  const [autostart, setAutostart] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setLocal(settings), [settings]);
  useEffect(() => {
    invoke<boolean>("get_autostart").then(setAutostart);
  }, []);

  type UrlCheck = "idle" | "checking" | "ok" | "warn" | "error";
  const [urlCheck, setUrlCheck] = useState<UrlCheck>(() => {
    if (!ollama) return "idle";
    if (!ollama.reachable) return "error";
    return ollama.configured_model_available ? "ok" : "warn";
  });
  const urlCheckTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (local.cleanup_level === "none") { setUrlCheck("idle"); return; }
    const url = local.ollama_url;
    const model = local.ollama_model;
    if (urlCheckTimer.current) clearTimeout(urlCheckTimer.current);
    urlCheckTimer.current = setTimeout(async () => {
      setUrlCheck("checking");
      try {
        const base = url.trim().replace(/\/$/, "");
        const resp = await fetch(`${base}/api/tags`, { signal: AbortSignal.timeout(3000) });
        if (!resp.ok) { setUrlCheck("error"); return; }
        const data = await resp.json() as { models: { name: string }[] };
        const names = (data.models ?? []).map((m: { name: string }) => m.name);
        const modelBase = model.replace(/:latest$/, "");
        const available = names.some((n: string) => n === model || n.replace(/:latest$/, "") === modelBase);
        setUrlCheck(available ? "ok" : "warn");
      } catch {
        setUrlCheck("error");
      }
      refreshStatus();
    }, 400);
    return () => { if (urlCheckTimer.current) clearTimeout(urlCheckTimer.current); };
  }, [local.ollama_url, local.ollama_model, local.cleanup_level]);

  const apply = async (next: AppSettings) => {
    setLocal(next);
    try {
      await saveSettings(next);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="space-y-5">
      <header className="rise">
        <h1 className="font-display text-[28px] font-semibold tracking-tight">Settings</h1>
        <p className="mt-1 text-sm text-(--color-ink-soft)">Changes save instantly.</p>
        {error && <p className="mt-1 text-xs text-red-600">{error}</p>}
      </header>

      <Section title="Speech to text" delay="rise-1">
        <Row label="Model" hint="Parakeet is fastest; Whisper covers ~99 languages">
          <select
            className={inputCls}
            value={local.stt_model_id}
            onChange={async (e) => {
              const id = e.target.value;
              setLocal({ ...local, stt_model_id: id });
              await invoke("set_stt_model", { modelId: id });
              refreshStatus();
            }}
          >
            {(sttStatus?.models ?? []).map((m) => (
              <option key={m.id} value={m.id}>
                {m.name} · {m.size_mb} MB{m.downloaded ? "" : " (downloads)"}
              </option>
            ))}
          </select>
        </Row>
      </Section>

      <Section title="AI polish" delay="rise-1">
        <div className="py-3">
          <div className="mb-2.5 text-sm font-medium">Cleanup level</div>
          <div className="grid grid-cols-4 gap-2">
            {CLEANUP_LEVELS.map((lvl) => (
              <button
                key={lvl.value}
                onClick={() => apply({ ...local, cleanup_level: lvl.value })}
                className={`rounded-lg border px-3 py-2.5 text-left transition-colors ${
                  local.cleanup_level === lvl.value
                    ? "border-(--color-accent) bg-emerald-50 ring-1 ring-(--color-accent)"
                    : "border-(--color-line) hover:border-(--color-accent)/60"
                }`}
              >
                <div className="text-sm font-semibold">{lvl.label}</div>
                <div className="mt-1 text-xs leading-snug text-(--color-ink-soft)">{lvl.description}</div>
              </button>
            ))}
          </div>
          {local.cleanup_level === "none" && (
            <p className="mt-2 text-xs text-(--color-ink-soft)">Ollama is not used when cleanup is off.</p>
          )}
        </div>
        <Row label="Clipboard substitution" hint='Trigger phrases: "from the clipboard", "from clipboard", "in the clipboard", "in clipboard", "paste from clipboard"'>
          <input
            type="checkbox"
            className="h-4 w-4"
            checked={local.clipboard_substitution_enabled}
            onChange={(e) => apply({ ...local, clipboard_substitution_enabled: e.target.checked })}
          />
        </Row>
        <Row label="Ollama model" hint={ollama?.reachable ? "Recommended: qwen3:4b-instruct" : undefined}>
          {ollama && ollama.models.length > 0 ? (
            <select
              className={inputCls}
              value={local.ollama_model}
              onChange={(e) => apply({ ...local, ollama_model: e.target.value })}
            >
              {!ollama.models.includes(local.ollama_model) && (
                <option value={local.ollama_model}>{local.ollama_model} (not pulled)</option>
              )}
              {ollama.models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          ) : (
            <input
              className={inputCls}
              value={local.ollama_model}
              onChange={(e) => setLocal({ ...local, ollama_model: e.target.value })}
              onBlur={() => apply(local)}
            />
          )}
        </Row>
        <Row label="Ollama URL">
          <div className="flex items-center gap-2">
            <input
              className={`${inputCls} w-56`}
              value={local.ollama_url}
              onChange={(e) => setLocal({ ...local, ollama_url: e.target.value })}
              onBlur={() => apply(local)}
            />
            {local.cleanup_level !== "none" && urlCheck !== "idle" && (
              <span
                className={`text-xs font-medium ${
                  urlCheck === "ok" ? "text-emerald-600" :
                  urlCheck === "warn" ? "text-amber-600" :
                  urlCheck === "error" ? "text-red-500" :
                  "text-(--color-ink-soft)"
                }`}
                title={
                  urlCheck === "warn" ? `Pull ${local.ollama_model} in Ollama` :
                  urlCheck === "error" ? "Unreachable" : undefined
                }
              >
                {urlCheck === "checking" ? "…" :
                 urlCheck === "ok" ? "✓" :
                 urlCheck === "warn" ? "⚠" : "✕"}
              </span>
            )}
          </div>
        </Row>
        {local.cleanup_level !== "none" && ollama && !ollama.reachable && (
          <div className="rounded-lg bg-amber-50 border border-amber-200 px-3 py-2.5 my-1 text-xs text-amber-800 space-y-1">
            <p className="font-medium">Ollama is not running — transcripts will be pasted as-is.</p>
            <p>Install Ollama at <span className="font-mono">ollama.com</span>, then pull the recommended model:</p>
            <pre className="font-mono bg-amber-100 rounded px-2 py-1 select-all">ollama pull qwen3:4b-instruct</pre>
            <p className="text-amber-700">Any instruct model works; qwen3:4b-instruct is a good balance of speed and quality on Apple Silicon.</p>
          </div>
        )}
      </Section>

      <Section title="Activation" delay="rise-2">
        <Row label="Hotkey" hint="Click, then press a key or combo (e.g. hold Option alone)">
          <HotkeyRecorder value={local.hotkey} onChange={(hotkey) => apply({ ...local, hotkey })} />
        </Row>
        <Row label="Mode">
          <select
            className={inputCls}
            value={local.activation_mode}
            onChange={(e) =>
              apply({ ...local, activation_mode: e.target.value as AppSettings["activation_mode"] })
            }
          >
            <option value="push_to_talk">Hold to talk</option>
            <option value="toggle">Press to toggle</option>
          </select>
        </Row>
        <Row label="Launch at login">
          <input
            type="checkbox"
            className="h-4 w-4"
            checked={autostart}
            onChange={async (e) => {
              setAutostart(e.target.checked);
              await invoke("set_autostart", { enabled: e.target.checked });
            }}
          />
        </Row>
      </Section>

      <Section title="Per-app style" delay="rise-3">
        <div className="py-3">
          <p className="text-xs text-(--color-ink-soft)">
            Code editors, terminals, Slack and Mail are detected automatically. Add overrides for
            anything else.
          </p>
          <div className="mt-3 space-y-2">
            {local.app_profiles.map((p, i) => (
              <div key={i} className="flex items-center gap-2">
                <input
                  className={`${inputCls} flex-1`}
                  value={p.bundle_id}
                  placeholder="bundle id, e.g. com.microsoft.VSCode"
                  onChange={(e) => {
                    const next = [...local.app_profiles];
                    next[i] = { ...p, bundle_id: e.target.value };
                    setLocal({ ...local, app_profiles: next });
                  }}
                  onBlur={() => apply(local)}
                />
                <select
                  className={inputCls}
                  value={p.mode}
                  onChange={(e) => {
                    const next = [...local.app_profiles];
                    next[i] = { ...p, mode: e.target.value as AppProfile["mode"] };
                    apply({ ...local, app_profiles: next });
                  }}
                >
                  <option value="code">Code</option>
                  <option value="messaging">Messaging</option>
                  <option value="email">Email</option>
                  <option value="prose">Prose</option>
                </select>
                <button
                  className="rounded-lg px-2 py-1.5 text-xs text-red-600 hover:bg-red-50"
                  onClick={() =>
                    apply({ ...local, app_profiles: local.app_profiles.filter((_, j) => j !== i) })
                  }
                >
                  Remove
                </button>
              </div>
            ))}
            <button
              className="rounded-lg border border-dashed border-(--color-line) px-3 py-1.5 text-xs text-(--color-ink-soft) hover:border-(--color-accent) hover:text-(--color-accent)"
              onClick={() =>
                apply({
                  ...local,
                  app_profiles: [
                    ...local.app_profiles,
                    { bundle_id: shared.lastResult?.app_bundle_id ?? "", mode: "prose" },
                  ],
                })
              }
            >
              + Add override
            </button>
          </div>
        </div>
      </Section>

      <Section title="Privacy" delay="rise-4">
        <Row label="Keep history" hint="Transcripts never leave this Mac either way">
          <select
            className={inputCls}
            value={local.history_retention}
            onChange={(e) =>
              apply({ ...local, history_retention: e.target.value as AppSettings["history_retention"] })
            }
          >
            <option value="forever">Forever</option>
            <option value="24h">24 hours</option>
            <option value="never">Don't store</option>
          </select>
        </Row>
      </Section>
    </div>
  );
}
