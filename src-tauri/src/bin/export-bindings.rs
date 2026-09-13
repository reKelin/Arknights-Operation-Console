use std::path::PathBuf;

fn main() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/generated/bindings.ts");
    arknights_operation_runner_lib::export_bindings(output)
        .expect("生成 Tauri TypeScript 绑定失败");
}
