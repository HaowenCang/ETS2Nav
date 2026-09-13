//! 构建脚本（P4R Batch 6B §17）。
//!
//! 除了 Tauri 的常规构建期生成，这里向链接器传 `/Brepro`。
//!
//! ## 为什么必须显式传
//!
//! MSVC 的 `link.exe` 默认把**链接时刻**写进 PE 的 COFF `TimeDateStamp`，并把它复制到
//! 调试目录的若干条目里。因此同一份源码、同一套工具链、同一组编译参数，从两个不同的
//! target 目录各构建一次，产物并不逐字节相同。
//!
//! 本仓库已经为 telemetry plugin 的 DLL 处理过同一问题（`build.bat` 的 `/Brepro`，
//! 见 P4R Batch 6a），但两个 Rust 二进制一直没有处理——**同一类缺陷在 Rust 侧被漏掉了**。
//! 实测：`ets2nav-desktop.exe` 两次独立构建相差 **20 字节**，全部集中在
//! COFF `TimeDateStamp`（0x100）、三个调试目录条目的时间戳，以及一个 16 字节 GUID；
//! 不含任何构建路径字符串，因此差异不是路径泄漏，就是时间戳。
//!
//! 这条差异直接影响 §17 的 payload 可复现性判定与候选二进制的冻结：只要两次打包之间发生
//! 重新链接，`desktop_exe.sha256` 就会变，进而 `bundle-manifest.json` 与整棵树的摘要都会变。
//! `/Brepro` 让链接器改用内容派生的确定性时间戳，从根上消除它。

fn main() {
    // `/Brepro` 只对 MSVC 工具有意义；其它平台不受影响，因此按目标平台条件发出。
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/Brepro");
    }
    tauri_build::build()
}
