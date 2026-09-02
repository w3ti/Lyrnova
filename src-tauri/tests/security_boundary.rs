use std::{fs, path::PathBuf};

use serde_json::Value;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(relative_path: &str) -> Value {
    let path = manifest_dir().join(relative_path);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&contents)
        .unwrap_or_else(|error| panic!("invalid JSON in {}: {error}", path.display()))
}

#[test]
fn capabilities_target_only_local_windows() {
    let capabilities = manifest_dir().join("capabilities");
    let mut checked = 0;

    for entry in fs::read_dir(&capabilities).expect("capabilities directory must exist") {
        let path = entry.expect("capability entry must be readable").path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let contents = fs::read_to_string(&path).expect("capability must be readable");
        let capability: Value = serde_json::from_str(&contents).expect("capability must be JSON");

        assert!(capability.get("remote").is_none());

        if let Some(windows) = capability.get("windows").and_then(Value::as_array) {
            assert!(
                windows.iter().all(|label| label.as_str() == Some("main")),
                "{} must target only the local main window",
                path.display()
            );
        }

        checked += 1;
    }

    assert!(checked > 0, "at least one local capability must be checked");
}

#[test]
fn frontend_window_controls_have_an_explicit_allowlist() {
    let capability = read_json("capabilities/main.json");
    let permissions = capability["permissions"]
        .as_array()
        .expect("permissions must be an array");
    let actual: Vec<_> = permissions.iter().filter_map(Value::as_str).collect();

    assert_eq!(
        actual,
        [
            "core:window:allow-close",
            "core:window:allow-maximize",
            "core:window:allow-minimize",
            "core:window:allow-start-dragging",
            "core:window:allow-toggle-maximize",
        ]
    );
}

#[test]
fn configuration_bootstraps_only_the_local_workspace() {
    let config = read_json("tauri.conf.json");
    let windows = config["app"]["windows"]
        .as_array()
        .expect("app.windows must be an array");
    let labels: Vec<_> = windows
        .iter()
        .filter_map(|window| window["label"].as_str())
        .collect();

    assert_eq!(labels, ["main"]);
}

#[test]
fn local_workspace_csp_denies_network_embeds_and_unsafe_scripts() {
    let config = read_json("tauri.conf.json");
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("security.csp must be a string");

    for directive in [
        "script-src 'self'",
        "worker-src 'self'",
        "connect-src 'none'",
        "object-src 'none'",
        "frame-src 'none'",
        "form-action 'none'",
    ] {
        assert!(csp.contains(directive), "CSP must contain {directive}");
    }

    assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
    assert!(!csp.contains("'unsafe-eval'"));
}

#[test]
fn local_workspace_uses_only_external_bundled_javascript() {
    let html = fs::read_to_string(manifest_dir().join("../ui/index.html"))
        .expect("local shell must be readable");
    let normalized = html.to_ascii_lowercase();

    assert!(normalized.contains("<script type=\"module\" src=\"/app.js\""));
    assert!(!normalized.contains("<script>"));
    assert!(!normalized.contains("javascript:"));
    assert!(!normalized.contains(" onload="));
}

#[test]
fn initial_shell_is_an_ide_without_ai_controls() {
    let html = fs::read_to_string(manifest_dir().join("../ui/index.html"))
        .expect("local shell must be readable");

    assert!(html.contains("data-ai-plugin-enabled=\"false\""));
    assert!(html.contains("data-activity=\"agent\"") && html.contains("ai-plugin-control"));
    assert!(html.contains("data-action=\"open-account\"") && html.contains("hidden>◎"));
    assert!(html.contains("data-action=\"show-agent-panel\" hidden"));
}

#[test]
fn local_workspace_has_no_remote_resources() {
    let ui_dir = manifest_dir().join("../ui");
    for name in ["index.html", "styles.css", "app.js"] {
        let contents = fs::read_to_string(ui_dir.join(name)).expect("UI asset must be readable");
        assert!(
            !contents.contains("https://"),
            "{name} loads a remote resource"
        );
        assert!(
            !contents.contains("http://"),
            "{name} loads a remote resource"
        );
    }
}
