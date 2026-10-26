#[tauri::command]
fn engine_version() -> &'static str {
    invectus_core::VERSION
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![engine_version])
        .run(tauri::generate_context!())
        .expect("failed to run inVectus");
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_uses_core() {
        assert_eq!(super::engine_version(), "0.1.0");
    }
}
