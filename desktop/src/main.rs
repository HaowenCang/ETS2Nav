// P4 Desktop（§58）：Tauri 2 最小壳——加载 nav-server 同源 web UI。
// 构建产物为独立窗口应用；server 独立进程（nav-core-cli server）提供数据。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("tauri app 运行失败");
}
