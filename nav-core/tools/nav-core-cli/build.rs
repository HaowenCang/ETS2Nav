//! 构建脚本（P4R Batch 6B §17）。
//!
//! 唯一作用是向 MSVC 链接器传 `/Brepro`。
//!
//! ## 为什么必须显式传
//!
//! `link.exe` 默认把**链接时刻**写进 PE 的 COFF `TimeDateStamp`，并复制到调试目录的若干
//! 条目里，因此同一份源码从两个不同的 target 目录各构建一次并不逐字节相同。
//! 本仓库已为 telemetry plugin 的 DLL 处理过同一问题（`build.bat` 的 `/Brepro`），
//! 但 Rust 二进制一直没有处理——同一类缺陷在 Rust 侧被漏掉了。
//!
//! sidecar 的字节身份会被写进 `bundle-manifest.json`（`sidecar.sha256`），并由 Desktop 在
//! 每次启动时校验，因此这条差异会同时破坏 §17 的 payload 可复现性与候选冻结的稳定性：
//! 只要两次打包之间发生重新链接，身份就会变。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/Brepro");
    }
}
