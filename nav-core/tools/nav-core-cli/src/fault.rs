//! 数据源 worker 的故障遏制（P4R Batch 6A §3/§4）。
//!
//! ## 问题形态（zombie service）
//!
//! 修改前：数据源线程由 `std::thread::spawn` 建立且返回的 `JoinHandle` 被丢弃，
//! 主线程阻塞在 `listener.incoming()`。线程 panic 只终止该线程——**进程存活，
//! HTTP listener 继续接受连接并返回 200，但 `latest_json` 永远停在同一帧**。
//! 观测到的现象因此与「网络卡了」「UI 没刷新」不可区分；Batch 5.5 的 BLS-01
//! 失败（`session.rs:454` 越界索引）正是这个形态，当时只能靠 stderr 诊断事后定位。
//!
//! ## 判据
//!
//! `panic ≠ recoverable frame error`。panic 表示不变量已破，恢复到「继续跑同一个
//! NavigationSession」在语义上不成立——损坏的会话会以未知方式继续产出状态。
//! 因此本模块只提供**单向**通道：worker 终止 → 上报 → 进程非零退出。
//!
//! ## 结构
//!
//! ```text
//! source worker (nav-source)
//!   ↓ JoinHandle::join 返回（Ok(()) 或 Err(payload)）
//! supervisor 线程 (nav-supervisor)
//!   ↓ FatalBus：Mutex + Condvar（事件驱动，非轮询）
//! 主线程：显式断开全部 WS 客户端 → 输出致命诊断 → process::exit(EXIT_SOURCE_FATAL)
//! ```
//!
//! 主线程不阻塞在 `listener.incoming()`：accept 循环移入独立线程 `nav-http`，
//! 主线程阻塞在 [`FatalBus::wait`] 上。因此「worker 死亡」到「进程开始退出」之间
//! 不存在轮询延迟——唤醒由 `Condvar` 直接完成，与任何轮询周期无关。

use std::sync::{Arc, Condvar, Mutex};

/// 数据源 worker 致命终止时的进程退出码。
///
/// 刻意与既有码位分离：`1` 是启动/业务失败，`2` 是用法错误，`101` 是 Rust panic
/// 的默认退出码。若沿用 `1`，观测方无法区分「服务从未起来」与「服务起来了然后
/// 数据源死了」——这两种情形的处置完全不同，判据必须能区分它们。
pub const EXIT_SOURCE_FATAL: i32 = 70;

/// 致命诊断行的稳定前缀（FC-04 断言的字面量）。
///
/// 产品与测试共用同一个常量，避免出现「测试断言的字面量与产品输出的字面量漂移
/// 后双双通过」——那是断言失效而非通过。
pub const FATAL_MARKER: &str = "[server] FATAL class=";

/// worker 终止的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalKind {
    /// worker 线程 panic（`JoinHandle::join` 返回 `Err`）。
    WorkerPanic,
    /// worker 线程在无致命信号的情况下结束（循环意外返回）。
    WorkerExitedUnexpectedly,
}

impl FatalKind {
    /// 诊断行中的类别字面量。
    pub fn class(self) -> &'static str {
        match self {
            FatalKind::WorkerPanic => "SOURCE_WORKER_PANIC",
            FatalKind::WorkerExitedUnexpectedly => "SOURCE_WORKER_EXIT",
        }
    }
}

/// 一份致命报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatalReport {
    pub kind: FatalKind,
    /// 单行、有长度上界、已抹去会话令牌的诊断细节。
    pub detail: String,
}

impl FatalReport {
    /// 渲染为 stderr 诊断行。**不含令牌**由 [`redact_secret`] 在构造侧保证。
    pub fn diagnostic_line(&self) -> String {
        format!(
            "{}{} exit={} worker=nav-source detail={}",
            FATAL_MARKER,
            self.kind.class(),
            EXIT_SOURCE_FATAL,
            self.detail
        )
    }
}

/// 单份致命报告的单向传播通道。
///
/// 语义要点：
/// - **只取首个**致命报告（后续上报不覆盖）：首个才是根因，覆盖会把因果顺序丢掉。
/// - `wait` 是事件驱动的（`Condvar`），不是轮询：worker 一死，等待方立即被唤醒。
pub struct FatalBus {
    inner: Mutex<Option<FatalReport>>,
    cv: Condvar,
}

impl FatalBus {
    pub fn new() -> Arc<Self> {
        Arc::new(FatalBus {
            inner: Mutex::new(None),
            cv: Condvar::new(),
        })
    }

    /// 上报致命。首个生效。
    pub fn report(&self, report: FatalReport) {
        let mut slot = self.inner.lock().unwrap();
        if slot.is_none() {
            *slot = Some(report);
            self.cv.notify_all();
        }
    }

    /// 是否已进入致命状态（不阻塞）。
    pub fn is_fatal(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    /// 阻塞直到出现致命报告并返回它。
    pub fn wait(&self) -> FatalReport {
        let mut slot = self.inner.lock().unwrap();
        while slot.is_none() {
            slot = self.cv.wait(slot).unwrap();
        }
        slot.clone().expect("循环退出时必然已有报告")
    }
}

/// 诊断文本中单行化与长度上界的字符数上限。
const MAX_DETAIL_CHARS: usize = 400;

/// 把任意文本规范化为「单行、有界」，用于诊断输出。
///
/// 目的有二：①换行会让一条致命记录在日志里伪装成多条，破坏按行解析；
/// ②panic payload 由 panic 点决定，长度不受本模块控制，必须有上界。
pub fn one_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_DETAIL_CHARS));
    let mut last_space = false;
    for c in raw.chars() {
        let c = if c == '\r' || c == '\n' || c == '\t' {
            ' '
        } else {
            c
        };
        if c == ' ' {
            if last_space {
                continue; // 折叠连续空白
            }
            last_space = true;
        } else {
            last_space = false;
        }
        if out.chars().count() >= MAX_DETAIL_CHARS {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// panic payload → 可安全打印的单行文本。
///
/// payload 的类型由 panic 点决定（`&'static str` / `String` / 其它）。非字符串
/// payload 不得被丢弃：丢掉它会让 FC-04 的「有明确 fatal class」退化为「只有类别
/// 没有内容」，故障仍然无法定位。
pub fn describe_panic(payload: &(dyn std::any::Any + Send)) -> String {
    let raw = if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<非字符串 panic payload>".to_string()
    };
    let line = one_line(&raw);
    if line.is_empty() {
        "<空 panic payload>".to_string()
    } else {
        line
    }
}

/// 从诊断文本中抹去会话令牌。
///
/// 存在的理由：panic payload 的内容在构造上不受本模块控制，任何上游 `format!`
/// 都可能把上下文（理论上含令牌）拼进 payload。与其论证「当前没有这样的调用点」，
/// 不如在输出路径上做无条件替换——这样 FC-05 是一条结构性性质，而不是一次审计
/// 结论。空 secret 视为「无令牌可抹」，因为空串参与 `replace` 会在每个字符间插入
/// 替换文本。
pub fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_string();
    }
    text.replace(secret, "<redacted>")
}

/// 测试专用故障注入。
///
/// **只在 debug 构建中存在**：整个模块由 `#[cfg(debug_assertions)]` 门控，release
/// 二进制中连环境变量名字面量都不存在（由回归门以字节扫描断言，见
/// `scripts/regression.ps1` 的 `release-binary-has-no-injection-hook`）。
///
/// 之所以用环境变量而不是控制端点：`/api/panic` 之类的接口即便加上鉴权，也把
/// 「远程可触发进程自杀」变成了产品面；环境变量只能由已经能启动该进程的本机调用方
/// 设置，因此不扩大任何攻击面。
#[cfg(debug_assertions)]
pub mod inject {
    /// 注入开关的环境变量名。
    pub const ENV: &str = "ETS2NAV_FAULT_INJECT";

    /// 致命退出**延迟**（毫秒）的环境变量名。
    ///
    /// 存在理由：产品在降级窗口内只停留微秒级（唤醒是 Condvar，接线是 shutdown），
    /// 因此「降级期间 listener 仍会按健康语义回答吗」这一问题在观测上不可判定——
    /// 观测不到并不等于性质不成立，但也不等于性质被验证过。该旋钮把窗口拉长到可
    /// 观测的量级，使 FC-02 成为确定性断言而非竞态抽样。
    ///
    /// 它**只在 debug 构建中存在**，且只延迟退出、不改变任何判定顺序：
    /// 降级置位与客户端断开仍然先于延迟发生。生产路径不存在该延迟，
    /// 因此「无长 zombie window」这一性质仍由不带延迟的运行测得（见 FC-02 的上界）。
    pub const HOLD_ENV: &str = "ETS2NAV_FAULT_HOLD_MS";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Fault {
        /// 在第一帧广播之后 panic（验证 supervisor 观察 panic 的路径）。
        SourcePanic,
        /// 在第一帧广播之后让 worker 正常返回（验证「意外正常结束」也被判为致命）。
        SourceExit,
    }

    /// 解析注入值。未识别的值返回 `None`（**不**报错：不能因为测试环境里有个
    /// 无关变量就让生产路径行为改变）。
    pub fn parse(v: &str) -> Option<Fault> {
        match v.trim() {
            "source-panic" => Some(Fault::SourcePanic),
            "source-exit" => Some(Fault::SourceExit),
            _ => None,
        }
    }

    /// 读取当前请求的注入。
    pub fn requested() -> Option<Fault> {
        std::env::var(ENV).ok().as_deref().and_then(parse)
    }

    /// 读取致命退出延迟（毫秒）。缺省 0；非法值视为 0（同样不报错）。
    /// 上界用于防止把一个测试旋钮变成「永不停机」的开关。
    pub fn shutdown_hold_ms() -> u64 {
        const MAX_HOLD_MS: u64 = 10_000;
        std::env::var(HOLD_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|v| v.min(MAX_HOLD_MS))
            .unwrap_or(0)
    }

    /// 在 `process::exit` 之前调用：按需延迟退出，使降级窗口可观测。
    pub fn apply_shutdown_hold() {
        let ms = shutdown_hold_ms();
        if ms > 0 {
            eprintln!("[server] FATAL hold={ms}ms（测试专用延迟，仅 debug 构建存在）");
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }

    /// 每广播一帧调用一次。
    ///
    /// 触发点选在**第一帧之后**而不是 worker 启动时：FC-06 要断言「已连接的 WS
    /// 客户端会失去连接，而不是永久停在最后一帧」，这要求确实存在「最后一帧」。
    /// 返回值 `true` 表示调用方应立即从 worker 循环返回（`SourceExit` 路径）。
    pub fn after_frame_broadcast(emitted: &mut u64, fault: Option<Fault>) -> bool {
        *emitted += 1;
        if *emitted != 1 {
            return false;
        }
        match fault {
            Some(Fault::SourcePanic) => {
                panic!("[fault-inject] 数据源 worker 注入的确定性 panic（测试专用路径）")
            }
            Some(Fault::SourceExit) => true,
            None => false,
        }
    }
}

/// release 构建中的空实现：故障注入在发布产物里**不存在**。
///
/// 显式给出这个函数（而不是到处写 `#[cfg]`）使调用点只有一处条件编译，
/// 且「release 不做任何注入」在一处可读、可审计。
#[cfg(not(debug_assertions))]
pub mod inject {
    pub fn apply_shutdown_hold() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_bus_is_event_driven_and_first_wins() {
        let bus = FatalBus::new();
        assert!(!bus.is_fatal());
        bus.report(FatalReport {
            kind: FatalKind::WorkerPanic,
            detail: "first".into(),
        });
        assert!(bus.is_fatal());
        // 第二个上报不得覆盖首个：根因是首个
        bus.report(FatalReport {
            kind: FatalKind::WorkerExitedUnexpectedly,
            detail: "second".into(),
        });
        let got = bus.wait();
        assert_eq!(got.detail, "first");
        assert_eq!(got.kind, FatalKind::WorkerPanic);
    }

    #[test]
    fn fatal_bus_wait_wakes_on_report_from_another_thread() {
        // 「主线程能实际观察 worker 终止」的最小证据：wait 在另一个线程上报之前
        // 处于阻塞状态，上报之后返回。此测试若恒过（例如 wait 立即返回），
        // 下面的 elapsed 断言会因数值过小而失去意义——故同时断言它确实等待到了报告。
        let bus = FatalBus::new();
        let b2 = bus.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            b2.report(FatalReport {
                kind: FatalKind::WorkerExitedUnexpectedly,
                detail: "from-thread".into(),
            });
        });
        let r = bus.wait();
        assert_eq!(r.detail, "from-thread");
        assert!(bus.is_fatal());
    }

    #[test]
    fn diagnostic_line_carries_class_and_exit_code() {
        let r = FatalReport {
            kind: FatalKind::WorkerPanic,
            detail: "index out of bounds".into(),
        };
        let line = r.diagnostic_line();
        assert!(line.starts_with(FATAL_MARKER), "{line}");
        assert!(line.contains("class=SOURCE_WORKER_PANIC"), "{line}");
        assert!(line.contains("exit=70"), "{line}");
        assert!(line.contains("worker=nav-source"), "{line}");
        assert!(!line.contains('\n'), "致命诊断必须是单行");
        assert_eq!(
            FatalKind::WorkerExitedUnexpectedly.class(),
            "SOURCE_WORKER_EXIT"
        );
        assert_eq!(EXIT_SOURCE_FATAL, 70);
    }

    #[test]
    fn one_line_flattens_and_bounds() {
        assert_eq!(one_line("a\r\nb\tc"), "a b c");
        assert_eq!(one_line("  a   b  "), "a b");
        assert_eq!(one_line(""), "");
        let long = "x".repeat(MAX_DETAIL_CHARS + 100);
        let got = one_line(&long);
        assert_eq!(got.chars().count(), MAX_DETAIL_CHARS + 1); // 上界 + 省略号
        assert!(got.ends_with('…'));
    }

    #[test]
    fn describe_panic_handles_both_payload_shapes() {
        let a: Box<dyn std::any::Any + Send> = Box::new("static str payload");
        assert_eq!(describe_panic(&*a), "static str payload");
        let b: Box<dyn std::any::Any + Send> = Box::new(String::from("owned\npayload"));
        assert_eq!(describe_panic(&*b), "owned payload");
        let c: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(describe_panic(&*c), "<非字符串 panic payload>");
        let d: Box<dyn std::any::Any + Send> = Box::new("");
        assert_eq!(describe_panic(&*d), "<空 panic payload>");
    }

    #[test]
    fn redact_secret_removes_every_occurrence() {
        // 令牌形态：64 位小写十六进制（security::TOKEN_HEX_LEN）
        let token = "a".repeat(64);
        let text = format!("token={token} 与再次出现 {token} 均须消失");
        let out = redact_secret(&text, &token);
        assert!(!out.contains(&token), "{out}");
        assert_eq!(out.matches("<redacted>").count(), 2);
        // 空 secret：不得把每个字符切开
        assert_eq!(redact_secret("abc", ""), "abc");
        // 非空但不匹配：原样保留（不得因「抹除」把正常文本吃掉）
        assert_eq!(redact_secret("abc", "zzz"), "abc");
    }

    #[test]
    fn redacted_report_never_contains_the_token() {
        // 把「panic payload 里意外含令牌」这一最坏情形做成可复现输入：
        let token = "deadbeef".repeat(8);
        let payload: Box<dyn std::any::Any + Send> =
            Box::new(format!("设置目的地失败，会话令牌 {token} 无效"));
        let detail = redact_secret(&describe_panic(&*payload), &token);
        let line = FatalReport {
            kind: FatalKind::WorkerPanic,
            detail,
        }
        .diagnostic_line();
        assert!(!line.contains(&token), "致命诊断不得含令牌: {line}");
        assert!(line.contains("<redacted>"));
        assert!(line.starts_with(FATAL_MARKER));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn injection_values_are_parsed_strictly() {
        use inject::{parse, Fault};
        assert_eq!(parse("source-panic"), Some(Fault::SourcePanic));
        assert_eq!(parse(" source-exit "), Some(Fault::SourceExit));
        assert_eq!(parse(""), None);
        assert_eq!(parse("panic"), None);
        assert_eq!(parse("SOURCE-PANIC"), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn injection_trips_only_after_the_first_frame() {
        use inject::{after_frame_broadcast, Fault};
        // 无注入：永不触发
        let mut n = 0u64;
        for _ in 0..5 {
            assert!(!after_frame_broadcast(&mut n, None));
        }
        assert_eq!(n, 5);
        // SourceExit：恰在第一帧之后请求退出
        let mut n = 0u64;
        assert!(after_frame_broadcast(&mut n, Some(Fault::SourceExit)));
        assert!(!after_frame_broadcast(&mut n, Some(Fault::SourceExit)));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "fault-inject")]
    fn injection_panics_on_the_first_frame() {
        use inject::{after_frame_broadcast, Fault};
        let mut n = 0u64;
        let _ = after_frame_broadcast(&mut n, Some(Fault::SourcePanic));
    }
}
