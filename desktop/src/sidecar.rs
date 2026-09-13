//! nav-core-cli sidecar 的启动、就绪判定与终止（P4R Batch 6A §5-§8）。
//!
//! ## 端口协商（§7）
//!
//! Desktop 以 `--port=0` 启动 sidecar，由内核分配端口，再从 sidecar 的 **stdout**
//! 读取 `[server] port=<n>`。这条通道的三点设计理由：
//!
//! - 端口必须能传，否则「8123 被占用」会变成「窗口打开了但永远连不上」；
//! - 走 stdout 而不是 stderr：stderr 是诊断流（人类可读、可能被日志系统改写与
//!   缓冲），stdout 是机器通道，`println!` 按行刷新；
//! - **令牌不经此通道**：令牌只经回环 `GET /api/bootstrap` 交付，页面与普通浏览器
//!   走完全相同的引导路径。Desktop 自身从不读取令牌，因此也没有「令牌经命令行 /
//!   环境变量 / URL query 泄漏」这条路径可被误用。
//!
//! ## 终止（§8）
//!
//! 三重保障，强度递减但互相独立：
//! 1. 作业对象 `KILL_ON_JOB_CLOSE`：Desktop 被强杀也不会留下 sidecar；
//! 2. `terminate()`：正常退出路径显式终止；
//! 3. `Child` 的 `wait()` 由监督线程阻塞等待，运行期意外退出可被立即观察。
//!
//! ## 作业对象不可用时的 fail closed（Batch 6B §4）
//!
//! 上述第 1 条是**发布性质**，不是优化项。作业对象建立或加入失败时，产品剩下的只有
//! 「窗口关闭时顺手杀掉子进程」，而 Desktop 被强杀（任务管理器、崩溃、断电式终止）后
//! 会留下一个仍在监听端口的 nav-server——界面消失、服务还在，正是本批次要消除的
//! zombie 形态之一。
//!
//! 因此缺省策略是**要求**作业对象：`create` 或 `assign` 失败即启动失败、立即回收
//! sidecar、非零退出，并且不开窗口。开发者可以用 `--allow-no-job-object` 显式降级，
//! 此时必须留下 WARN——降级是产品性质的丧失，不能是静默行为。
//!
//! 判据不是「是否 debug 构建」而是「调用方是否显式声明可以不要作业对象」：debug 与
//! release 走同一条 fail-closed 判据，因为「这次运行是不是发布形态」与「这次运行能不
//! 能保证清理」是两个无关的问题。

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::log::{err, out};
use crate::winproc::{Job, ProcessRef};

/// 就绪等待上界。数据集加载 + search.db 读取在冷缓存上可能远超 10 s
/// （E2E harness 记录过 45 s 以上），因此与 harness 取同一量级。
const READY_TIMEOUT: Duration = Duration::from_secs(120);
/// 单次 HTTP 探测的往返上界。
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// 启动参数。
pub struct SpawnSpec<'a> {
    pub exe: &'a Path,
    pub dataset: &'a Path,
    pub web_root: &'a Path,
    /// 是否把服务暴露到局域网。默认关闭：桌面应用不得在用户未要求时扩大本机网络暴露面。
    pub lan: bool,
    /// 回放录制轨迹（透传 `--replay`）。`None` 表示等待实时遥测。
    pub replay: Option<&'a Path>,
    /// 注入合成灯态剧本（透传 `--fake-signal`）。
    pub fake_signal: bool,
    /// 是否**要求**作业对象接管 sidecar（§4）。
    ///
    /// 缺省应为 `true`。为 `false` 时作业对象失败只降级并告警，仅限开发者显式请求。
    pub job_required: bool,
}

/// 启动失败。区分「作业对象不可用」与其余原因，使产品退出码能反映**哪一类**发布性质
/// 未能建立——把两者都折叠成一个「启动失败」会让「作业对象没生效」在发布检查里不可观测。
#[derive(Debug)]
pub enum StartError {
    /// §4：本次启动要求作业对象，但创建或加入失败。
    JobRequired { step: String, detail: String },
    /// 其余启动/就绪失败。
    Other(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::JobRequired { step, detail } => {
                write!(f, "作业对象不可用（step={step}）: {detail}")
            }
            StartError::Other(e) => write!(f, "{e}"),
        }
    }
}

/// 已启动的 sidecar。
pub struct Sidecar {
    pub exe: PathBuf,
    pub pid: u32,
    pub port: u16,
    /// 从启动到 bootstrap 返回 200 的耗时。
    pub ready_ms: u128,
    proc: Option<ProcessRef>,
    job: Option<Job>,
    exited: Receiver<std::process::ExitStatus>,
    /// 已取出的退出状态。监督线程一旦结束就会投递，取出后缓存在此处，
    /// 使「等待退出」与「停机」可以被分别调用而不会把状态丢掉。
    exited_status: Option<std::process::ExitStatus>,
    /// 本次停机是否已发起：监督线程据此区分「我们杀的子进程」与「子进程自己死了」。
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 从 sidecar 的 stdout 行中解析端口。
///
/// 契约是**精确**的：`[server] port=<十进制>`。刻意不做「宽松匹配任意数字」——
/// 宽松匹配会把某天多出来的一行日志误判成端口，从而连到错误的端口上。
pub fn parse_port_line(line: &str) -> Option<u16> {
    let rest = line.trim().strip_prefix("[server] port=")?;
    rest.trim().parse::<u16>().ok()
}

/// 以一次最小 HTTP GET 判断 bootstrap 是否已可用。返回 HTTP 状态码。
///
/// 不使用 HTTP 客户端库：本模块只需要读状态行，而引入一个客户端会为「判断服务是否
/// 起来」这一个用途增加依赖面。请求不带 Origin，因此服务端按原生客户端处理
/// （这正是 curl 的路径，也是 bootstrap 在无 Origin 时的既定分支）。
fn probe_bootstrap(port: u16) -> Option<u16> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    s.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    s.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
    let req = format!(
        "GET /api/bootstrap HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > 8192 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let first = head.lines().next()?;
    let mut parts = first.split_whitespace();
    let _http = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

/// fail closed：立即终止刚启动的 sidecar、回收它，并返回作业对象失败。
///
/// 此处的显式 `terminate` 不可省略。`assign` 失败意味着 sidecar **不在**作业内，关闭
/// 作业句柄对它没有任何作用——Batch 6A 的变异实验 M2a 正是在这一点上推翻了「作业对象
/// 可以接管显式终止」的假设：移除显式终止后，作业对象只在句柄关闭时才动手，而句柄关闭
/// 发生在停机流程之后，于是正常停机收不回子进程。两种机制覆盖的是**不同**性质。
fn reap_and_fail(
    child: &mut std::process::Child,
    proc: Option<&ProcessRef>,
    step: &str,
    detail: String,
) -> StartError {
    if let Some(p) = proc {
        p.terminate(1);
    }
    let _ = child.kill();
    let _ = child.wait();
    StartError::JobRequired {
        step: step.to_string(),
        detail,
    }
}

impl Sidecar {
    /// 启动并按契约等待就绪。
    ///
    /// `on_unexpected_exit` 在监督线程中被调用一次：sidecar 在**非本次停机**的情况下
    /// 结束（panic、被外部终止、自行退出）时触发。它是 D-06/D-07 的唯一入口——
    /// 没有它，sidecar 死掉后界面会停在最后一帧且没有任何人知道。
    pub fn start<F>(spec: &SpawnSpec, on_unexpected_exit: F) -> Result<Sidecar, StartError>
    where
        F: FnOnce(std::process::ExitStatus) + Send + 'static,
    {
        // 作业先建：必须在子进程创建之后立刻加入，避免「创建成功但加入前父进程死亡」
        // 的窗口尽可能小。加入动作紧随 spawn，中间不做任何阻塞操作。
        //
        // 创建失败时**尚未派生任何子进程**，fail-closed 路径因此没有需要回收的对象；
        // 这正是 create 与 assign 两种失败处置不同、且必须分别记录 step 的原因。
        let job = match Job::create_kill_on_close() {
            Ok(j) => Some(j),
            Err(e) => {
                if spec.job_required {
                    return Err(StartError::JobRequired {
                        step: "create".to_string(),
                        detail: e.to_string(),
                    });
                }
                err(&format!(
                    "[desktop] WARN kind=job-create-failed detail={e}（--allow-no-job-object：sidecar 清理降级为正常退出路径；Desktop 被强杀时可能残留 sidecar）"
                ));
                None
            }
        };

        let mut cmd = Command::new(spec.exe);
        cmd.arg("server")
            .arg(spec.dataset)
            .arg("--port=0")
            .arg(format!("--web={}", spec.web_root.display()));
        if spec.lan {
            cmd.arg("--lan");
        }
        if let Some(t) = spec.replay {
            cmd.arg(format!("--replay={}", t.display()));
        }
        if spec.fake_signal {
            cmd.arg("--fake-signal");
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(spec.exe.parent().unwrap_or(Path::new(".")));
        let mut child = cmd.spawn().map_err(|e| {
            StartError::Other(format!("启动 sidecar 失败（{}）: {e}", spec.exe.display()))
        })?;
        let pid = child.id();

        let proc = ProcessRef::open(pid);
        let job_state = match (job.as_ref(), proc.as_ref()) {
            (Some(j), Some(p)) => match j.assign(p) {
                Ok(()) => "assigned",
                Err(code) => {
                    let detail = format!("AssignProcessToJobObject win32_error={code}");
                    if spec.job_required {
                        return Err(reap_and_fail(&mut child, proc.as_ref(), "assign", detail));
                    }
                    err(&format!(
                        "[desktop] WARN kind=job-assign-failed win32_error={code}（--allow-no-job-object：sidecar 清理降级为正常退出路径）"
                    ));
                    "assign-failed"
                }
            },
            (Some(_), None) => {
                // 作业已建立但进程句柄打不开 ⇒ 同样无法加入，其后果与 assign 失败一致。
                // 旧实现把这一支漏在 `else if job.is_none()` 之外，于是「句柄打不开」既
                // 没有告警也没有判据——一条静默的降级路径。
                let detail = format!("OpenProcess(pid={pid}) 失败，无法把 sidecar 加入作业对象");
                if spec.job_required {
                    return Err(reap_and_fail(&mut child, None, "open-process", detail));
                }
                err(&format!(
                    "[desktop] WARN kind=job-assign-failed step=open-process detail={detail}"
                ));
                "assign-failed"
            }
            (None, _) => "create-failed",
        };
        out(&format!(
            "[desktop] sidecar-job state={job_state} required={} pid={pid}",
            spec.job_required
        ));

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| StartError::Other("sidecar stdout 不可用".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| StartError::Other("sidecar stderr 不可用".to_string()))?;

        // stderr 逐行转发到 Desktop 的 stderr（保留 server 自己的诊断原文）。
        std::thread::Builder::new()
            .name("sidecar-stderr".to_string())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    err(&format!("[sidecar] {line}"));
                }
            })
            .map_err(|e| StartError::Other(format!("创建 stderr 转发线程失败: {e}")))?;

        // stdout：解析端口行，其余行原样转发。
        let (port_tx, port_rx) = mpsc::channel::<u16>();
        std::thread::Builder::new()
            .name("sidecar-stdout".to_string())
            .spawn(move || {
                let mut sent = false;
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if !sent {
                        if let Some(p) = parse_port_line(&line) {
                            let _ = port_tx.send(p);
                            sent = true;
                            continue;
                        }
                    }
                    out(&format!("[sidecar] {line}"));
                }
            })
            .map_err(|e| StartError::Other(format!("创建 stdout 解析线程失败: {e}")))?;

        let started = Instant::now();
        let port = match port_rx.recv_timeout(READY_TIMEOUT) {
            Ok(p) => p,
            Err(RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                return Err(StartError::Other(format!(
                    "sidecar 未在 {} s 内报告端口（stdout 未出现 \"[server] port=<n>\"）",
                    READY_TIMEOUT.as_secs()
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                return Err(StartError::Other(
                    "sidecar 在报告端口之前即结束（见其 stderr 诊断）".to_string(),
                ));
            }
        };

        // 就绪判据是 bootstrap 返回 200：端口已绑定不等于「已可服务」
        // （server 在 bind 之后仍要读取 search.db）。
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut last = None;
        let ready = loop {
            if let Some(code) = probe_bootstrap(port) {
                if code == 200 {
                    break true;
                }
                last = Some(code);
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if !ready {
            if let Some(p) = proc.as_ref() {
                p.terminate(1);
            }
            return Err(StartError::Other(format!(
                "sidecar 已在 :{port} 报告端口，但 bootstrap 在 {} s 内未返回 200（最后一次 status={last:?}）",
                READY_TIMEOUT.as_secs()
            )));
        }
        let ready_ms = started.elapsed().as_millis();

        // 监督线程：阻塞等待子进程结束（不是轮询）。停机期间由 `stopped` 抑制回调。
        let (exit_tx, exit_rx) = mpsc::channel();
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped_for_thread = stopped.clone();
        let mut child_for_wait = child;
        std::thread::Builder::new()
            .name("sidecar-supervisor".to_string())
            .spawn(move || {
                let status = child_for_wait.wait();
                match status {
                    Ok(st) => {
                        let _ = exit_tx.send(st);
                        if !stopped_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                            on_unexpected_exit(st);
                        }
                    }
                    Err(e) => {
                        err(&format!(
                            "[desktop] ERROR kind=sidecar-wait-failed detail={e}"
                        ));
                    }
                }
            })
            .map_err(|e| StartError::Other(format!("创建 sidecar 监督线程失败: {e}")))?;

        Ok(Sidecar {
            exe: spec.exe.to_path_buf(),
            pid,
            port,
            ready_ms,
            proc,
            job,
            exited: exit_rx,
            exited_status: None,
            stopped,
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// 阻塞等待 sidecar 结束，最多 `timeout`。返回其退出状态；超时返回 `None`。
    ///
    /// 这是**阻塞等待**而不是轮询：监督线程在子进程结束时立即投递状态，此调用
    /// 直接阻塞在该通道上，因此「sidecar 死了」到「Desktop 知道」之间没有轮询周期。
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<std::process::ExitStatus> {
        if let Some(st) = self.exited_status {
            return Some(st);
        }
        match self.exited.recv_timeout(timeout) {
            Ok(st) => {
                self.exited_status = Some(st);
                Some(st)
            }
            Err(_) => None,
        }
    }

    /// 显式终止并回收。返回是否观察到退出。
    pub fn shutdown(&mut self, timeout: Duration) -> bool {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(p) = self.proc.as_ref() {
            p.terminate(0);
        }
        let got = self.wait_exit(timeout).is_some();
        // 作业句柄在此处释放；KILL_ON_JOB_CLOSE 是最后一道保证——即便显式终止失败
        // （句柄权限不足、进程已僵死），句柄关闭时内核仍会终止作业成员。
        self.job = None;
        self.proc = None;
        got
    }

    /// sidecar 是否仍在运行（不阻塞）。
    pub fn is_running(&mut self) -> bool {
        if self.exited_status.is_some() {
            return false;
        }
        self.exited
            .try_recv()
            .map(|st| self.exited_status = Some(st))
            .is_err()
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        // 未走停机流程就被丢弃（例如构造失败路径或提前 return）时仍要终止子进程。
        // 这是第三重保障，前两重是显式 terminate 与作业对象。
        if self.proc.is_some() {
            if let Some(p) = self.proc.as_ref() {
                p.terminate(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_line_contract_is_exact() {
        assert_eq!(parse_port_line("[server] port=8123"), Some(8123));
        assert_eq!(parse_port_line("  [server] port=1  "), Some(1));
        assert_eq!(parse_port_line("[server] port=65535"), Some(65535));
        // 非契约行不得被宽松匹配成端口
        assert_eq!(parse_port_line("listening on 127.0.0.1:8123"), None);
        assert_eq!(parse_port_line("[server] port="), None);
        assert_eq!(parse_port_line("[server] port=abc"), None);
        assert_eq!(parse_port_line("[server] port=70000"), None);
        assert_eq!(parse_port_line("[server] ports=80"), None);
        assert_eq!(parse_port_line(""), None);
    }

    #[test]
    fn probe_against_closed_port_is_none() {
        // 关闭的端口：connect 必须失败并返回 None，而不是 panic 或挂起
        // （选一个几乎不可能被占用的高位端口）
        assert!(probe_bootstrap(1).is_none());
    }
}
