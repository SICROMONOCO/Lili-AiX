fn main() {
    #[cfg(target_os = "linux")]
    tauri_build::build();
}
