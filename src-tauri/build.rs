fn main() {
    if std::env::var_os("EXPORT_BINDINGS").is_some() {
        return;
    }
    tauri_build::build()
}
