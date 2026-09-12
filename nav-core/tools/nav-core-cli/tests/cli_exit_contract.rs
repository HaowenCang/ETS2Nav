//! nav-core-cli 的机器契约回归（P4R Batch 5 §16 / §37）。
//!
//! 覆盖范围刻意限制在**不依赖任何外部输入**的路径：用法错误与坐标解析错误必须由
//! 退出码表达，且不得以 panic 的形式表达。panic 的退出码是 101，虽然同样非零，
//! 但它把「用法错误」在形态上伪装成「程序缺陷」，机器无法据退出码区分二者，人也
//! 必须读 stderr 才能判断——这正是 Batch 4 要求消除的判定含混。
//!
//! 依赖 dataset 的业务路径（有路线 → 0；无路线 → 非零）无法在此确定性构造，
//! 由回归 harness 在真实数据集上以真实退出码断言；纯判定逻辑另由
//! `main.rs` 内的 `route_exit_code_*` 单元测试覆盖。

use std::process::Command;

/// Cargo 为同 package 的集成测试提供被测二进制的绝对路径。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_nav-core-cli")
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

fn run(args: &[&str]) -> Run {
    let o = Command::new(bin())
        .args(args)
        .output()
        .expect("无法启动 nav-core-cli");
    Run {
        code: o.status.code().expect("进程被信号终止（不应发生）"),
        out: String::from_utf8_lossy(&o.stdout).to_string(),
        err: String::from_utf8_lossy(&o.stderr).to_string(),
    }
}

/// 用法错误的统一判据：退出码符合预期、非 panic、有 stderr 诊断、stdout 不带错误文本。
fn assert_clean_usage_error(r: &Run, expect_code: i32, ctx: &str) {
    assert_eq!(r.code, expect_code, "{ctx}: 退出码不符 (stderr={})", r.err);
    assert_ne!(r.code, 101, "{ctx}: 不得以 panic 表达用法错误");
    assert!(
        !r.err.contains("panicked"),
        "{ctx}: stderr 出现 panic: {}",
        r.err
    );
    assert!(!r.err.trim().is_empty(), "{ctx}: 用法错误必须给出诊断信息");
    assert!(
        r.out.trim().is_empty(),
        "{ctx}: 错误信息不得写进 stdout（机器可读输出须保持干净）: {}",
        r.out
    );
}

#[test]
fn no_subcommand_is_usage_error() {
    assert_clean_usage_error(&run(&[]), 2, "无子命令");
}

#[test]
fn route_with_too_few_arguments_is_usage_error() {
    // 参数不足时落到 main 的 `_` 分支打印用法
    assert_clean_usage_error(&run(&["route"]), 2, "route 无参数");
    assert_clean_usage_error(&run(&["route", "1,2:3,4"]), 2, "route 缺 dataset");
}

/// 坐标解析必须发生在读取 dataset 之前，因此这里故意传入不存在的 dataset 路径：
/// 若解析检查被放到 dataset 加载之后，本测试会因加载失败而得到不同的错误，
/// 从而暴露顺序退化。
#[test]
fn route_format_errors_are_usage_errors_before_any_dataset_io() {
    let no_dataset = if cfg!(windows) {
        r"C:\ETS2Nav-nonexistent-dataset-for-arg-check"
    } else {
        "/nonexistent/ets2nav-dataset-for-arg-check"
    };

    // 缺冒号：字段数不足
    assert_clean_usage_error(&run(&["route", "nonsense", no_dataset]), 2, "缺冒号");
    // 冒号过多
    assert_clean_usage_error(&run(&["route", "1,2:3,4:5,6", no_dataset]), 2, "冒号过多");
    // 单侧坐标字段过多：原实现对 v[1] 之外的多余字段不报错，改用严格解析后必须拒绝
    assert_clean_usage_error(&run(&["route", "1,2,3:4,5", no_dataset]), 2, "起点字段过多");
    // 非数值坐标：原实现 parse().unwrap() 在此 panic（exit 101）
    assert_clean_usage_error(&run(&["route", "a,b:c,d", no_dataset]), 2, "非数值坐标");
}
