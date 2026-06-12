import { useState } from "react";
import type { Shared } from "../App";

export default function Dictionary({ shared }: { shared: Shared }) {
  const { settings, saveSettings } = shared;
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = async () => {
    const words = draft
      .split(/[,\n]/)
      .map((w) => w.trim())
      .filter((w) => w && !settings.dictionary.includes(w));
    if (words.length === 0) return;
    try {
      await saveSettings({ ...settings, dictionary: [...settings.dictionary, ...words] });
      setDraft("");
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const remove = async (word: string) => {
    await saveSettings({
      ...settings,
      dictionary: settings.dictionary.filter((w) => w !== word),
    });
  };

  return (
    <div className="space-y-5">
      <header className="rise">
        <h1 className="font-display text-[28px] font-semibold tracking-tight">Dictionary</h1>
        <p className="mt-1 text-sm text-(--color-ink-soft)">
          Names and jargon the polish step should spell exactly — teammates, products, acronyms.
        </p>
      </header>

      <div className="rise rise-1 flex gap-2">
        <input
          className="flex-1 rounded-lg border border-(--color-line) bg-(--color-card) px-3 py-2 text-sm outline-none placeholder:text-(--color-ink-soft)/60 focus:border-(--color-accent)"
          placeholder="Add a word (or several, comma-separated)…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <button
          className="rounded-lg bg-(--color-accent) px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-40"
          disabled={!draft.trim()}
          onClick={add}
        >
          Add
        </button>
      </div>
      {error && <p className="text-xs text-red-600">{error}</p>}

      {settings.dictionary.length === 0 ? (
        <p className="rise rise-2 py-10 text-center text-sm text-(--color-ink-soft)">
          Nothing yet. Try adding the words people always misspell.
        </p>
      ) : (
        <div className="rise rise-2 flex flex-wrap gap-2">
          {settings.dictionary.map((w) => (
            <span
              key={w}
              className="group flex items-center gap-1.5 rounded-full border border-(--color-line) bg-(--color-card) py-1 pr-2 pl-3 text-sm shadow-[0_1px_2px_rgb(0_0_0/0.03)]"
            >
              {w}
              <button
                className="flex h-4 w-4 items-center justify-center rounded-full text-(--color-ink-soft) hover:bg-red-50 hover:text-red-600"
                onClick={() => remove(w)}
                aria-label={`Remove ${w}`}
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
