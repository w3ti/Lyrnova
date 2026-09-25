//! Install a local test plugin using the production installer into an empty,
//! explicitly marked E2E profile. Never used by the application itself.
use std::{fs, path::PathBuf};

use lyrnova_lib::plugin_package::{PluginPackageDescriptor, PluginPackageInstaller};
use semver::Version;
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("missing fixture root")?);
    if !root.join(".lyrnova-e2e-fixture").is_file() {
        return Err("refusing a directory without the E2E marker".into());
    }
    let config = root.join("config/io.github.w3ti.lyrnova");
    if config.join("plugins").exists() || config.join("plugins.json").exists() {
        return Err("plugin profile must be empty".into());
    }
    let manifest = include_bytes!("../../tests/e2e/plugin.json");
    let runtime = include_bytes!("../../tests/e2e/plugin.py");
    let mut archive = tar::Builder::new(Vec::new());
    for (path, bytes) in [
        ("plugin.json", manifest.as_slice()),
        ("runtime.py", runtime.as_slice()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append_data(&mut header, path, bytes)?;
    }
    let compressed = zstd::stream::encode_all(archive.into_inner()?.as_slice(), 1)?;
    let package = root.join("e2e-plugin.tar.zst");
    fs::write(&package, &compressed)?;
    let descriptor = PluginPackageDescriptor {
        asset: "e2e-plugin.tar.zst".into(),
        sha256: format!("{:x}", Sha256::digest(&compressed)),
    };
    let installer = PluginPackageInstaller::new(config.join("plugins"), Version::new(0, 1, 0));
    let staged = installer
        .stage_local(&package, descriptor)
        .map_err(|e| format!("stage: {e:?}"))?;
    let permissions = staged.review().manifest.permissions.clone();
    staged
        .install(&permissions)
        .map_err(|e| format!("install: {e:?}"))?;
    // Installation/grants are fixture setup. Activation is exercised through UI.
    fs::write(
        config.join("plugins.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 4,
            "installed": {"io.github.w3ti.lyrnova.tool.e2e": "0.1.0"},
            "enabled": [],
            "grants": {"io.github.w3ti.lyrnova.tool.e2e": permissions}
        }))?,
    )?;
    Ok(())
}
