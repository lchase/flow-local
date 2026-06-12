use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrontmostApp {
    pub name: String,
    pub bundle_id: String,
}

/// Frontmost app via NSWorkspace — needs no permission, unlike window titles
/// (Screen Recording) or AX reads (Accessibility).
#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<FrontmostApp> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let name = app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let bundle_id = app
        .bundleIdentifier()
        .map(|s| s.to_string())
        .unwrap_or_default();
    Some(FrontmostApp { name, bundle_id })
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<FrontmostApp> {
    None
}
