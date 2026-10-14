use std::sync::Mutex;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::pipeline::DictationResult;

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Clone, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub ts: String,
    pub app_name: Option<String>,
    pub app_bundle_id: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub cleaned: bool,
    pub audio_seconds: f64,
    pub stt_ms: i64,
    pub llm_ms: Option<i64>,
    pub words: i64,
}

#[derive(Clone, Serialize)]
pub struct Stats {
    pub total_dictations: i64,
    pub total_words: i64,
    pub total_audio_seconds: f64,
    /// Average speaking rate across dictations (words per minute of audio).
    pub avg_wpm: f64,
    /// Minutes saved vs typing the same words at 45 wpm.
    pub minutes_saved: f64,
    pub streak_days: i64,
    pub distinct_apps: i64,
    pub days_active: i64,
    pub per_app: Vec<AppCount>,
}

#[derive(Clone, Serialize)]
pub struct AppCount {
    pub app_name: String,
    pub dictations: i64,
    pub words: i64,
}

#[derive(Clone, Serialize)]
pub struct Achievement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub unlocked_at: Option<String>,
}

struct AchievementDef {
    id: &'static str,
    title: &'static str,
    description: &'static str,
}

const ACHIEVEMENTS: &[AchievementDef] = &[
    AchievementDef { id: "first_words", title: "First Words", description: "Complete your first dictation." },
    AchievementDef { id: "chatterbox", title: "Chatterbox", description: "Dictate 1,000 words." },
    AchievementDef { id: "wordsmith", title: "Wordsmith", description: "Dictate 10,000 words." },
    AchievementDef { id: "novelist", title: "Novelist", description: "Dictate 100,000 words." },
    AchievementDef { id: "streak_3", title: "Warming Up", description: "Dictate 3 days in a row." },
    AchievementDef { id: "streak_7", title: "Habit Formed", description: "Dictate 7 days in a row." },
    AchievementDef { id: "fast_talker", title: "Fast Talker", description: "Average over 150 wpm in a dictation of 15+ seconds." },
    AchievementDef { id: "marathon", title: "Marathon", description: "A single dictation over 60 seconds." },
    AchievementDef { id: "everywhere", title: "Everywhere", description: "Dictate into 5 different apps." },
    AchievementDef { id: "centurion", title: "Centurion", description: "Complete 100 dictations." },
    // Streak milestones
    AchievementDef { id: "streak_14", title: "On a Roll", description: "Dictate 14 days in a row." },
    AchievementDef { id: "streak_30", title: "Creature of Habit", description: "Dictate 30 days in a row." },
    // Volume milestones
    AchievementDef { id: "dictation_machine", title: "Dictation Machine", description: "Complete 1,000 dictations." },
    AchievementDef { id: "half_million", title: "Half Million", description: "Dictate 500,000 words." },
    // Time saved
    AchievementDef { id: "time_saver", title: "Time Saver", description: "Save 60 minutes vs. typing." },
    AchievementDef { id: "day_saver", title: "Day Saver", description: "Save 8 hours vs. typing." },
    // App diversity
    AchievementDef { id: "world_tour", title: "World Tour", description: "Dictate into 10 different apps." },
    // Speed
    AchievementDef { id: "speed_demon", title: "Speed Demon", description: "Average over 200 wpm in a dictation of 15+ seconds." },
    AchievementDef { id: "quick_draw", title: "Quick Draw", description: "STT completes in under 500 ms on a 10+ word dictation." },
    // Time of day
    AchievementDef { id: "night_owl", title: "Night Owl", description: "Dictate between midnight and 4 am." },
    AchievementDef { id: "early_bird", title: "Early Bird", description: "Dictate between 4 am and 6 am." },
    // Quality
    AchievementDef { id: "no_edits", title: "No Edits Needed", description: "AI cleanup applied but changed nothing (10+ words)." },
];

impl Store {
    pub fn open(app: &AppHandle) -> Result<Self> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| anyhow!("no app data dir: {e}"))?;
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("flow-local.db"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                app_name TEXT,
                app_bundle_id TEXT,
                raw_text TEXT NOT NULL,
                final_text TEXT NOT NULL,
                cleaned INTEGER NOT NULL,
                audio_seconds REAL NOT NULL,
                stt_ms INTEGER NOT NULL,
                llm_ms INTEGER,
                words INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS achievements (
                id TEXT PRIMARY KEY,
                unlocked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Record a dictation and return any newly unlocked achievements.
    pub fn record_dictation(&self, r: &DictationResult) -> Result<Vec<Achievement>> {
        let words = r.final_text.split_whitespace().count() as i64;
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO history
                 (app_name, app_bundle_id, raw_text, final_text, cleaned, audio_seconds, stt_ms, llm_ms, words)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    r.app_name,
                    r.app_bundle_id,
                    r.raw_text,
                    r.final_text,
                    r.cleaned,
                    r.audio_seconds,
                    r.stt_ms as i64,
                    r.llm_ms.map(|v| v as i64),
                    words
                ],
            )?;
        }
        self.check_achievements()
    }

    pub fn history(&self, search: Option<String>, limit: i64) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", search.unwrap_or_default());
        let mut stmt = conn.prepare(
            "SELECT id, ts, app_name, app_bundle_id, raw_text, final_text, cleaned,
                    audio_seconds, stt_ms, llm_ms, words
             FROM history
             WHERE final_text LIKE ?1 OR raw_text LIKE ?1 OR app_name LIKE ?1
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    ts: row.get(1)?,
                    app_name: row.get(2)?,
                    app_bundle_id: row.get(3)?,
                    raw_text: row.get(4)?,
                    final_text: row.get(5)?,
                    cleaned: row.get(6)?,
                    audio_seconds: row.get(7)?,
                    stt_ms: row.get(8)?,
                    llm_ms: row.get(9)?,
                    words: row.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_entry(&self, id: i64) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM history", [])?;
        Ok(())
    }

    /// Apply the retention setting; "24h" prunes older rows, "never" removes all.
    pub fn apply_retention(&self, retention: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        match retention {
            "24h" => {
                conn.execute(
                    "DELETE FROM history WHERE ts < strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 day')",
                    [],
                )?;
            }
            "never" => {
                conn.execute("DELETE FROM history", [])?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn stats(&self) -> Result<Stats> {
        let conn = self.conn.lock().unwrap();
        let (total_dictations, total_words, total_audio_seconds): (i64, i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(words),0), COALESCE(SUM(audio_seconds),0) FROM history",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
        let avg_wpm = if total_audio_seconds > 0.0 {
            total_words as f64 / (total_audio_seconds / 60.0)
        } else {
            0.0
        };
        // Time saved = typing time at 45wpm minus speaking time.
        let minutes_saved = (total_words as f64 / 45.0) - (total_audio_seconds / 60.0);

        let distinct_apps: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT app_bundle_id) FROM history WHERE app_bundle_id IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let days_active: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT date(ts,'localtime')) FROM history",
            [],
            |r| r.get(0),
        )?;

        // Streak: consecutive days ending today (or yesterday, so an unbroken
        // streak doesn't show 0 before today's first dictation).
        let days: Vec<String> = conn
            .prepare("SELECT DISTINCT date(ts,'localtime') FROM history ORDER BY 1 DESC")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let streak_days = compute_streak(&days);

        let per_app = conn
            .prepare(
                "SELECT COALESCE(app_name,'Unknown'), COUNT(*), SUM(words)
                 FROM history GROUP BY app_name ORDER BY COUNT(*) DESC LIMIT 10",
            )?
            .query_map([], |r| {
                Ok(AppCount {
                    app_name: r.get(0)?,
                    dictations: r.get(1)?,
                    words: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(Stats {
            total_dictations,
            total_words,
            total_audio_seconds,
            avg_wpm,
            minutes_saved: minutes_saved.max(0.0),
            streak_days,
            distinct_apps,
            days_active,
            per_app,
        })
    }

    pub fn achievements(&self) -> Result<Vec<Achievement>> {
        let conn = self.conn.lock().unwrap();
        let mut unlocked = std::collections::HashMap::new();
        let mut stmt = conn.prepare("SELECT id, unlocked_at FROM achievements")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, at) = row?;
            unlocked.insert(id, at);
        }
        Ok(ACHIEVEMENTS
            .iter()
            .map(|d| Achievement {
                id: d.id.to_string(),
                title: d.title.to_string(),
                description: d.description.to_string(),
                unlocked_at: unlocked.get(d.id).cloned(),
            })
            .collect())
    }

    fn check_achievements(&self) -> Result<Vec<Achievement>> {
        let stats = self.stats()?;
        let conn = self.conn.lock().unwrap();
        let fast_talker: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history WHERE audio_seconds >= 15
                AND words / (audio_seconds / 60.0) > 150)",
            [],
            |r| r.get(0),
        )?;
        let marathon: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history WHERE audio_seconds > 60)",
            [],
            |r| r.get(0),
        )?;
        let speed_demon: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history WHERE audio_seconds >= 15
                AND words / (audio_seconds / 60.0) > 200)",
            [],
            |r| r.get(0),
        )?;
        let quick_draw: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history WHERE words >= 10 AND stt_ms < 500)",
            [],
            |r| r.get(0),
        )?;
        let night_owl: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history
                WHERE CAST(strftime('%H', ts, 'localtime') AS INTEGER) BETWEEN 0 AND 3)",
            [],
            |r| r.get(0),
        )?;
        let early_bird: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history
                WHERE CAST(strftime('%H', ts, 'localtime') AS INTEGER) BETWEEN 4 AND 5)",
            [],
            |r| r.get(0),
        )?;
        let no_edits: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history
                WHERE cleaned = 1 AND raw_text = final_text AND words >= 10)",
            [],
            |r| r.get(0),
        )?;

        let earned = |id: &str| -> bool {
            match id {
                "first_words" => stats.total_dictations >= 1,
                "chatterbox" => stats.total_words >= 1_000,
                "wordsmith" => stats.total_words >= 10_000,
                "novelist" => stats.total_words >= 100_000,
                "streak_3" => stats.streak_days >= 3,
                "streak_7" => stats.streak_days >= 7,
                "fast_talker" => fast_talker,
                "marathon" => marathon,
                "everywhere" => stats.distinct_apps >= 5,
                "centurion" => stats.total_dictations >= 100,
                "streak_14" => stats.streak_days >= 14,
                "streak_30" => stats.streak_days >= 30,
                "dictation_machine" => stats.total_dictations >= 1_000,
                "half_million" => stats.total_words >= 500_000,
                "time_saver" => stats.minutes_saved >= 60.0,
                "day_saver" => stats.minutes_saved >= 480.0,
                "world_tour" => stats.distinct_apps >= 10,
                "speed_demon" => speed_demon,
                "quick_draw" => quick_draw,
                "night_owl" => night_owl,
                "early_bird" => early_bird,
                "no_edits" => no_edits,
                _ => false,
            }
        };

        let mut newly = Vec::new();
        for def in ACHIEVEMENTS {
            if !earned(def.id) {
                continue;
            }
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO achievements (id) VALUES (?1)",
                params![def.id],
            )?;
            if inserted > 0 {
                newly.push(Achievement {
                    id: def.id.to_string(),
                    title: def.title.to_string(),
                    description: def.description.to_string(),
                    unlocked_at: Some("now".to_string()),
                });
            }
        }
        Ok(newly)
    }
}

fn compute_streak(days_desc: &[String]) -> i64 {
    use chrono::{Duration, Local, NaiveDate};
    let today = Local::now().date_naive();
    let mut expected = today;
    let mut streak = 0i64;
    for (i, day) in days_desc.iter().enumerate() {
        let Ok(date) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
            break;
        };
        if i == 0 && date == today - Duration::days(1) {
            // No dictation yet today; streak continues from yesterday.
            expected = date;
        }
        if date == expected {
            streak += 1;
            expected -= Duration::days(1);
        } else {
            break;
        }
    }
    streak
}
