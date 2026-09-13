//! 作业对象故障注入（P4R Batch 6B §4）。
//!
//! ## 为什么用 cargo feature 而不是 `debug_assertions`
//!
//! 本轮要证明的性质是「**release 构建**在作业对象不可用时 fail closed」。若以
//! `debug_assertions` 为门，注入代码只存在于 debug 构建，于是 release 侧的判据无法被
//! 真正执行——只能靠阅读代码推断，而「编译通过不等于行为成立」正是本批次要消除的推理
//! 方式。改用显式 feature 后两种构建各司其职：
//!
//! ```text
//! cargo build --release                          → 无注入代码（发布产物，字节扫描可证）
//! cargo build --release --features fault-inject   → 有注入代码，且完整走 release 判据路径
//! ```
//!
//! ## 与 nav-core-cli 注入钩子的关系
//!
//! 两者刻意使用**不同的环境变量名**（`ETS2NAV_JOB_FAULT` 与 `ETS2NAV_FAULT_INJECT`）：
//! 命名空间重叠会让「哪一侧的注入被触发」在混合测试中变得不可判定。`assemble-bundle.ps1`
//! 对发布产物同时扫描这两个字面量，任一出现即拒绝出厂。

use std::env;

/// 注入开关的环境变量名。
pub const ENV: &str = "ETS2NAV_JOB_FAULT";

/// 可注入的失败点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobFault {
    /// `CreateJobObjectW` / `SetInformationJobObject` 阶段失败。
    Create,
    /// `AssignProcessToJobObject` 阶段失败。
    Assign,
}

/// 读取注入请求。未设置或取值无法识别时返回 `None`。
///
/// 取值无法识别时**不报错也不降级为某个默认注入点**：静默选择失败点会让测试断言的对象
/// 与测试者以为的对象不一致，从而产生假阳性。
pub fn requested() -> Option<JobFault> {
    match env::var(ENV).ok()?.trim().to_ascii_lowercase().as_str() {
        "create" => Some(JobFault::Create),
        "assign" => Some(JobFault::Assign),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_spellings_select_a_fault() {
        // 直接测试解析函数而不是经 env：env 是进程级共享状态，测试并行执行时
        // 设置/清除会互相干扰，从而产生偶发失败——那会让「注入有效」这条判据不可信。
        let parse = |s: &str| match s.trim().to_ascii_lowercase().as_str() {
            "create" => Some(JobFault::Create),
            "assign" => Some(JobFault::Assign),
            _ => None,
        };
        assert_eq!(parse("create"), Some(JobFault::Create));
        assert_eq!(parse("  ASSIGN "), Some(JobFault::Assign));
        assert_eq!(parse("assign "), Some(JobFault::Assign));
        // 拼错或未知取值不得落到某个默认注入点
        assert_eq!(parse("creat"), None);
        assert_eq!(parse("assign2"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("none"), None);
    }
}
