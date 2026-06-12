import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Shared } from "../App";
import type { AppProfile, AppSettings } from "../types";

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

export default function Settings({ shared }: { shared: Shared }) {
  const { settings, saveSettings, sttStatus, ollama, refreshStatus } = shared;
  const [local, setLocal] = useState<AppSettings>(settings);
  const [autostart, setAutostart] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setLocal(settings), [settings]);
  useEffect(() => {
    invoke<boolean>("get_autostart").then(setAutostart);
  }, []);

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
        <Row label="Clean up transcripts" hint="Falls back to the raw transcript if Ollama is unavailable">
          <input
            type="checkbox"
            className="h-4 w-4"
            checked={local.cleanup_enabled}
            onChange={(e) => apply({ ...local, cleanup_enabled: e.target.checked })}
          />
        </Row>
        <Row label="Ollama model">
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
          <input
            className={`${inputCls} w-56`}
            value={local.ollama_url}
            onChange={(e) => setLocal({ ...local, ollama_url: e.target.value })}
            onBlur={() => apply(local)}
          />
        </Row>
      </Section>

      <Section title="Activation" delay="rise-2">
        <Row label="Hotkey" hint='e.g. "alt+space", "cmd+shift+d"'>
          <input
            className={`${inputCls} w-40`}
            value={local.hotkey}
            onChange={(e) => setLocal({ ...local, hotkey: e.target.value })}
            onBlur={() => apply(local)}
          />
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
