mod audio;
mod cleanup;
mod context;
mod inject;
mod pipeline;
mod settings;
mod stt;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use audio::Recorder;
use inject::Injector;
use pipeline::Pipeline;
use settings::{AppSettings, SettingsStore};
use stt::Transcriber;

#[tauri::command]
fn get_settings(store: tauri::State<SettingsStore>) -> AppSettings {
    store.get()
}

#[tauri::command]
fn set_settings(
    app: AppHandle,
    store: tauri::State<SettingsStore>,
    settings: AppSettings,
) -> Result<(), String> {
    let old_hotkey = store.get().hotkey.clone();
    store.set(settings.clone()).map_err(|e| e.to_string())?;
    if settings.hotkey != old_hotkey {
        register_hotkey(&app, &settings.hotkey).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn stt_model_status(app: AppHandle, transcriber: tauri::State<Transcriber>) -> serde_json::Value {
    let downloaded = stt::model_path(&app).map(|p| p.exists()).unwrap_or(false);
    serde_json::json!({
        "model_id": stt::PARAKEET_V3.id,
        "model_name": stt::PARAKEET_V3.name,
        "downloaded": downloaded,
        "loaded": transcriber.is_loaded(),
    })
}

#[tauri::command]
async fn ollama_status(app: AppHandle) -> cleanup::OllamaStatus {
    let settings = app.state::<SettingsStore>().get();
    cleanup::ollama_status(&settings).await
}

#[tauri::command]
fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_accessibility_client::accessibility::application_is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[tauri::command]
fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_accessibility_client::accessibility::application_is_trusted_with_prompt()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[tauri::command]
fn mic_level(recorder: tauri::State<Recorder>) -> f32 {
    recorder.current_level()
}

fn register_hotkey(app: &AppHandle, binding: &str) -> anyhow::Result<()> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all()?;
    let shortcut: Shortcut = binding
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid hotkey '{binding}': {e:?}"))?;
    shortcuts.register(shortcut)?;
    log::info!("hotkey registered: {binding}");
    Ok(())
}

fn show_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &AppHandle) -> anyhow::Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Flow Local", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .icon_as_template(true)
        .tooltip("Flow Local")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => show_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    let pressed = event.state() == ShortcutState::Pressed;
                    let pipeline = app.state::<Pipeline>();
                    pipeline.on_hotkey(app, pressed);
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            stt_model_status,
            ollama_status,
            check_accessibility,
            request_accessibility,
            mic_level,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Menu-bar app: no dock icon.
            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let store = SettingsStore::load(&handle)?;
            let hotkey = store.get().hotkey.clone();
            app.manage(store);
            app.manage(Recorder::new());
            app.manage(Transcriber::new());
            app.manage(Injector::new());
            app.manage(Pipeline::new());

            setup_tray(&handle)?;
            register_hotkey(&handle, &hotkey)?;

            // Download/load the STT model and warm the Ollama model in the background.
            let bg = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = stt::ensure_model_ready(bg.clone()).await {
                    log::error!("STT model setup failed: {e}");
                }
                let settings = bg.state::<SettingsStore>().get();
                if settings.cleanup_enabled {
                    cleanup::warm_model(&settings).await;
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
