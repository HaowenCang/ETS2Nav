//! Desktop 的输出通道（P4R Batch 6A §19）。
//!
//! ## 为什么不用 `println!` / `eprintln!`
//!
//! release 构建带 `windows_subsystem = "windows"`：从资源管理器启动时进程没有控制台，
//! 标准句柄无效。此时 `println!` 的写入会失败，而 Rust 的 `print!` 系列在写入失败时
//! **panic**——一个日志语句会把「双击图标启动」变成崩溃。
//!
//! 因此所有输出都经本模块：写入失败一律忽略。副作用是 stdout 在无重定向时静默丢弃，
//! 这正是 GUI 应用应有的行为；而自动化检查总是重定向 stdout，因此机器通道仍然可用
//! （Windows 上 GUI 子系统进程同样会继承父进程显式提供的管道句柄）。
//!
//! ## 通道语义
//!
//! ```text
//! stdout  机器可读的生命周期记录（key=value，每行一条）
//! stderr  诊断与错误（含 kind= 与 code=）
//! ```
//!
//! 令牌绝不进入任一通道。

use std::io::Write;

/// 写一行到 stdout（失败即忽略）。
pub fn out(line: &str) {
    let mut h = std::io::stdout();
    let _ = writeln!(h, "{line}");
    let _ = h.flush();
}

/// 写一行到 stderr（失败即忽略）。
pub fn err(line: &str) {
    let mut h = std::io::stderr();
    let _ = writeln!(h, "{line}");
    let _ = h.flush();
}
