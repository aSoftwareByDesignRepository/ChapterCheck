include!("src/ipc_allowlist.rs");

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(APP_IPC_COMMANDS),
        ),
    )
    .expect("tauri-build failed");
}
