//! ETS2Nav Desktop —— 桌面产品边界（P4R Batch 6A §5-§9）。
//!
//! ## 架构选择：WebView 加载 sidecar 自己的回环 URL
//!
//! ```text
//! ets2nav-desktop.exe
//!   └─ spawn <bundle>/nav-core-cli.exe server <dataset> --port=0 --web=<bundle>/web
//!        └─ 在 127.0.0.1:<运行时端口> 上同时提供 UI 与 API/WS
//!  Tauri WebView
//!   └─ 加载 http://127.0.0.1:<运行时端口>/
//! ```
//!
//! 之所以选这个模式而不是「`tauri.localhost` 静态前端 + 跨源访问回环 API」：
//!
//! - 页面与 API **同源**，于是 bootstrap、WS、`/api/*` 的安全模型与普通浏览器完全
//!   一致。跨源模型需要 CORS 许可、需要把令牌跨源交给页面，是一条只服务于桌面壳的
//!   额外信任关系；
//! - 前端无需任何改动即可工作：`app.js` 已按 `location.host` 推导同源 WS 地址，
//!   桌面形态因此直接复用被 E2E 套件反复覆盖的那条路径（index.html 中的 8123 只是
//!   `file://` / `tauri.localhost` 情形的回退默认值，本模式下不会用到）；
//! - 端口在运行时协商，不存在「8123 被占用就永远连不上」的固定端口假设。
//!
//! 代价是 bundle 必须包含前端产物与 sidecar，且窗口创建要等 sidecar 就绪——
//! 这正是本轮要建立的发布边界：Desktop 不再是「薄壳 + 用户手工起服务」。
//!
//! ## 退出码契约
//!
//! ```text
//! 0   正常关闭
//! 2   用法错误
//! 20  打包缺陷（sidecar 缺失 / 身份不符 / 前端产物缺失）
//! 21  数据集缺失或不完整
//! 22  sidecar 启动或就绪失败
//! 23  sidecar 运行期意外退出
//! ```
//!
//! 与 `nav-core-cli` 的 0/1/2/70 及 harness 的 0/1/3/4 都不重叠，因此「桌面壳失败」
//! 与「服务本身失败」在退出码层面可区分。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bundle;
mod log;
mod sha256;
mod sidecar;
mod winproc;

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use log::{err, out};
use sidecar::{Sidecar, SpawnSpec};

const EXIT_OK: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_PACKAGING: i32 = 20;
const EXIT_DATASET: i32 = 21;
const EXIT_SIDECAR_START: i32 = 22;
const EXIT_SIDECAR_FATAL: i32 = 23;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Options {
    dataset: Option<PathBuf>,
    web: Option<PathBuf>,
    lan: bool,
    no_window: bool,
    hold_ms: u64,
    sidecar_override: Option<PathBuf>,
    verify_dataset_digest: bool,
    replay: Option<PathBuf>,
    fake_signal: bool,
}

fn usage() {
    err("用法: ets2nav-desktop [--dataset <dir>] [--web <dir>] [--lan] [--no-window] [--hold-ms <n>]");
    err("  --dataset <dir>  覆盖随包数据集目录（缺省 <bundle>/data/europe-v5）");
    err("  --web <dir>      覆盖随包前端目录（缺省 <bundle>/web）");
    err("  --lan            让 sidecar 同时监听局域网（缺省仅回环）");
    err("  --no-window      只跑生命周期、不开窗口（自动化检查用）");
    err("  --hold-ms <n>    --no-window 下保持运行的时长，缺省 1500");
    err("  --verify-dataset-digest  启动时重算整棵数据集树摘要并与清单比对（约 373 MB，默认关闭）");
    err("  --replay <trace> 让 sidecar 回放录制轨迹而不是等待实时遥测（演示/自动化用，透传给 server）");
    err("  --fake-signal    让 sidecar 注入合成灯态剧本（演示/自动化用，透传给 server）");
    err("退出码: 0 正常；2 用法；20 打包缺陷；21 数据集；22 sidecar 启动失败；23 sidecar 意外退出");
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        dataset: None,
        web: None,
        lan: false,
        no_window: false,
        hold_ms: 1500,
        sidecar_override: None,
        verify_dataset_digest: false,
        replay: None,
        fake_signal: false,
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let mut take_value = |name: &str| -> Result<String, String> {
            if let Some(v) = a.strip_prefix(&format!("{name}=")) {
                return Ok(v.to_string());
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} 缺少取值"))
        };
        match a.split('=').next().unwrap_or(a) {
            "--dataset" => o.dataset = Some(PathBuf::from(take_value("--dataset")?)),
            "--web" => o.web = Some(PathBuf::from(take_value("--web")?)),
            "--sidecar" => o.sidecar_override = Some(PathBuf::from(take_value("--sidecar")?)),
            "--lan" => o.lan = true,
            "--no-window" => o.no_window = true,
            "--verify-dataset-digest" => o.verify_dataset_digest = true,
            "--replay" => o.replay = Some(PathBuf::from(take_value("--replay")?)),
            "--fake-signal" => o.fake_signal = true,
            "--hold-ms" => {
                let v = take_value("--hold-ms")?;
                o.hold_ms = v
                    .parse::<u64>()
                    .map_err(|_| format!("--hold-ms 取值非法: {v}"))?;
            }
            "--help" | "-h" => {
                usage();
                std::process::exit(EXIT_OK);
            }
            other => return Err(format!("未知参数: {other}")),
        }
        i += 1;
    }
    Ok(o)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match parse_args(&args) {
        Ok(o) => run(o),
        Err(e) => {
            err(&format!("[desktop] ERROR kind=usage detail={e}"));
            usage();
            EXIT_USAGE
        }
    };
    std::process::exit(code);
}

/// 解析并校验 bundle 内容，返回 (sidecar 路径, 数据集, 前端目录)。
fn resolve_bundle(
    o: &Options,
) -> Result<(PathBuf, PathBuf, PathBuf, Option<bundle::Manifest>), i32> {
    let root = bundle::bundle_root();
    let manifest = match bundle::Manifest::load(&root) {
        Ok(m) => m,
        Err(e) => {
            err(&format!(
                "[desktop] ERROR kind=manifest-invalid code={EXIT_PACKAGING} detail={e}"
            ));
            return Err(EXIT_PACKAGING);
        }
    };
    out(&format!(
        "[desktop] version={APP_VERSION} bundle_root={}",
        root.display()
    ));

    // ── sidecar 身份（§6）───────────────────────────────────────────────────
    let sidecar_path = match o
        .sidecar_override
        .clone()
        .or_else(|| bundle::resolve_sidecar(&root, manifest.as_ref()))
    {
        Some(p) => p,
        None => {
            err(&format!(
                "[desktop] ERROR kind=sidecar-missing code={EXIT_PACKAGING} detail=在 {} 下未找到 {}.exe",
                root.display(),
                bundle::SIDECAR_BASE
            ));
            return Err(EXIT_PACKAGING);
        }
    };
    let (identity, sha, bytes) = match bundle::verify_sidecar(&sidecar_path, manifest.as_ref()) {
        Ok(v) => v,
        Err(e) => {
            err(&format!(
                "[desktop] ERROR kind=sidecar-identity-read code={EXIT_PACKAGING} detail={e}"
            ));
            return Err(EXIT_PACKAGING);
        }
    };
    let commit = manifest
        .as_ref()
        .and_then(|m| m.source_commit())
        .unwrap_or("unknown")
        .to_string();
    out(&format!(
        "[desktop] sidecar path={} sha256={sha} bytes={bytes} source_commit={commit} identity={}",
        sidecar_path.display(),
        identity.label()
    ));
    match &identity {
        bundle::SidecarIdentity::Verified => {}
        bundle::SidecarIdentity::ManifestMissing if cfg!(debug_assertions) => {
            err("[desktop] WARN kind=identity-unverified detail=缺少 bundle-manifest.json（debug 构建允许，release 构建为打包缺陷）");
        }
        bundle::SidecarIdentity::ManifestMissing => {
            err(&format!(
                "[desktop] ERROR kind=manifest-missing code={EXIT_PACKAGING} detail=release 构建必须随包提供 {}",
                bundle::MANIFEST_NAME
            ));
            return Err(EXIT_PACKAGING);
        }
        bundle::SidecarIdentity::Mismatch { expected, actual } => {
            // 身份不符是**打包缺陷**，不是可恢复错误：继续启动等于启动一个身份未知的
            // 可执行文件，「随包 binary」这一发布性质当场失效。
            err(&format!(
                "[desktop] ERROR kind=sidecar-identity-mismatch code={EXIT_PACKAGING} expected={expected} actual={actual}"
            ));
            return Err(EXIT_PACKAGING);
        }
    }

    // ── 前端产物（§10 的一半：UI 必须随包）──────────────────────────────────
    let web = o
        .web
        .clone()
        .unwrap_or_else(|| root.join(bundle::WEB_DIR_NAME));
    if !web.join("index.html").is_file() {
        err(&format!(
            "[desktop] ERROR kind=web-missing code={EXIT_PACKAGING} detail=前端产物缺失: {}",
            web.join("index.html").display()
        ));
        return Err(EXIT_PACKAGING);
    }
    out(&format!("[desktop] web root={}", web.display()));

    // ── 数据集（§9）─────────────────────────────────────────────────────────
    let dataset = o
        .dataset
        .clone()
        .unwrap_or_else(|| root.join(bundle::DATASET_REL));
    let check = match bundle::inspect_dataset(&dataset) {
        Ok(c) => c,
        Err(e) => {
            err(&format!(
                "[desktop] ERROR kind=dataset-missing code={EXIT_DATASET} detail={e}"
            ));
            return Err(EXIT_DATASET);
        }
    };
    // 清单描述的是**本 bundle**：只有使用随包数据集（未覆盖路径）时才以清单计数为准。
    // `--dataset` 是诊断/开发用途，此时只做结构检查——否则「指向另一个数据集」会被
    // 误报成「数据集不完整」，而这条判据的真实含义是「随包内容与清单不符」。
    let problems = if o.dataset.is_some() {
        check.problems(None)
    } else {
        check.problems(manifest.as_ref())
    };
    out(&format!(
        "[desktop] dataset path={} files={} bytes={} content_fingerprint={}",
        dataset.display(),
        check.files,
        check.bytes,
        manifest
            .as_ref()
            .and_then(|m| m.dataset_content_fingerprint())
            .unwrap_or("unknown")
    ));
    // 树摘要默认不重算（373 MB ⇒ 秒级启动延迟），此处的记录是**清单声明值**，
    // 不是本次实测值；实测值只在 --verify-dataset-digest 下产生并另行打印。
    out(&format!(
        "[desktop] dataset-declared tree_sha256={} files={} bytes={} (未在本次启动重算)",
        manifest
            .as_ref()
            .and_then(|m| m.dataset_tree_sha256())
            .unwrap_or("unknown"),
        manifest
            .as_ref()
            .and_then(|m| m.dataset_files())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        manifest
            .as_ref()
            .and_then(|m| m.dataset_bytes())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    ));
    if !problems.is_empty() {
        err(&format!(
            "[desktop] ERROR kind=dataset-incomplete code={EXIT_DATASET} detail={}",
            problems.join("; ")
        ));
        return Err(EXIT_DATASET);
    }
    if o.verify_dataset_digest {
        match bundle::dataset_tree_digest(&dataset) {
            Ok((digest, files, bytes)) => {
                let declared = manifest.as_ref().and_then(|m| m.dataset_tree_sha256());
                out(&format!(
                    "[desktop] dataset-digest computed={digest} files={files} bytes={bytes}"
                ));
                match declared {
                    Some(d) if d.eq_ignore_ascii_case(&digest) => {
                        out("[desktop] dataset-digest result=verified");
                    }
                    Some(d) => {
                        err(&format!(
                            "[desktop] ERROR kind=dataset-digest-mismatch code={EXIT_DATASET} expected={d} actual={digest}"
                        ));
                        return Err(EXIT_DATASET);
                    }
                    None => {
                        err(&format!(
                            "[desktop] ERROR kind=dataset-digest-unverifiable code={EXIT_DATASET} detail=清单未声明 dataset.tree_sha256"
                        ));
                        return Err(EXIT_DATASET);
                    }
                }
            }
            Err(e) => {
                err(&format!(
                    "[desktop] ERROR kind=dataset-digest-failed code={EXIT_DATASET} detail={e}"
                ));
                return Err(EXIT_DATASET);
            }
        }
    }
    Ok((sidecar_path, dataset, web, manifest))
}

fn run(o: Options) -> i32 {
    out(&format!(
        "[desktop] start version={APP_VERSION} no_window={} lan={}",
        o.no_window, o.lan
    ));
    let (sidecar_path, dataset, web, manifest) = match resolve_bundle(&o) {
        Ok(v) => v,
        Err(code) => return code,
    };

    // 运行期 sidecar 意外退出 → 记录原因并要求事件循环按 23 退出（D-06/D-07）。
    let exit_code = Arc::new(AtomicI32::new(EXIT_OK));
    let app_handle: Arc<OnceLock<tauri::AppHandle>> = Arc::new(OnceLock::new());
    let exit_code_for_watch = exit_code.clone();
    let handle_for_watch = app_handle.clone();
    let on_unexpected_exit = move |status: std::process::ExitStatus| {
        let code = status.code().unwrap_or(-1);
        err(&format!(
            "[desktop] ERROR kind=sidecar-exited code={EXIT_SIDECAR_FATAL} sidecar_exit={code} detail=sidecar 在本次停机之外结束"
        ));
        exit_code_for_watch.store(EXIT_SIDECAR_FATAL, Ordering::SeqCst);
        if let Some(h) = handle_for_watch.get() {
            // 关掉窗口：界面不得停留在「已连接但状态冻结」的形态。
            h.exit(EXIT_SIDECAR_FATAL);
        }
    };

    let mut sidecar = match Sidecar::start(
        &SpawnSpec {
            exe: &sidecar_path,
            dataset: &dataset,
            web_root: &web,
            lan: o.lan,
            replay: o.replay.as_deref(),
            fake_signal: o.fake_signal,
        },
        on_unexpected_exit,
    ) {
        Ok(s) => s,
        Err(e) => {
            err(&format!(
                "[desktop] ERROR kind=sidecar-start code={EXIT_SIDECAR_START} detail={e}"
            ));
            return EXIT_SIDECAR_START;
        }
    };
    out(&format!(
        "[desktop] sidecar-started pid={} exe={}",
        sidecar.pid,
        sidecar.exe.display()
    ));
    out(&format!(
        "[desktop] server port={} ready_ms={} url={}",
        sidecar.port,
        sidecar.ready_ms,
        sidecar.url()
    ));
    out(&format!(
        "[desktop] bundle basemap={} fonts={}",
        manifest
            .as_ref()
            .map(|m| m.basemap_present())
            .unwrap_or(false),
        manifest
            .as_ref()
            .map(|m| m.fonts_present())
            .unwrap_or(false)
    ));

    if o.no_window {
        // 无窗口模式：仍走完整生命周期（身份校验、启动、就绪、停机、回收），
        // 只省去 WebView。自动化检查用它验证产品边界中与界面无关的部分。
        out(&format!("[desktop] hold begin ms={}", o.hold_ms));
        // 阻塞等待「保持时长」或「sidecar 提前结束」二者之一，而不是 sleep 满时长：
        // 后者会让 sidecar 在保持期内死亡的情形被静默吞掉，最后以一个假的成功退出码
        // 收场——那正是本轮要消除的「看起来健康」。
        if let Some(status) = sidecar.wait_exit(Duration::from_millis(o.hold_ms)) {
            let code = status.code().unwrap_or(-1);
            err(&format!(
                "[desktop] ERROR kind=sidecar-died-during-hold code={EXIT_SIDECAR_FATAL} sidecar_exit={code}"
            ));
            out(&format!(
                "[desktop] shutdown reason=sidecar-died reaped=true sidecar_running={}",
                sidecar.is_running()
            ));
            return EXIT_SIDECAR_FATAL;
        }
        out("[desktop] hold end");
        let reaped = sidecar.shutdown(Duration::from_secs(10));
        // 自我核对：停机后 sidecar 不得仍在运行。同一条性质也由外部检查
        // （按 pid 查进程是否消失）独立验证——自证不足以作为唯一证据。
        let still_running = sidecar.is_running();
        out(&format!(
            "[desktop] shutdown reason=no-window reaped={reaped} sidecar_running={still_running}"
        ));
        return if reaped && !still_running {
            EXIT_OK
        } else {
            EXIT_SIDECAR_FATAL
        };
    }

    let url = sidecar.url();
    let parsed = match url.parse() {
        Ok(u) => u,
        Err(e) => {
            err(&format!(
                "[desktop] ERROR kind=url-invalid code={EXIT_SIDECAR_START} detail={e}"
            ));
            return EXIT_SIDECAR_START;
        }
    };
    let handle_slot = app_handle.clone();
    let built = tauri::Builder::default()
        .setup(move |app| {
            let _ = handle_slot.set(app.handle().clone());
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(parsed))
                .title("ETS2Nav")
                .inner_size(1280.0, 800.0)
                .on_page_load(|_w, payload| {
                    out(&format!(
                        "[desktop] page_load state={:?} url={}",
                        payload.event(),
                        payload.url()
                    ));
                })
                .build()?;
            out("[desktop] window created");
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(a) => a,
        Err(e) => {
            err(&format!(
                "[desktop] ERROR kind=window-build code={EXIT_SIDECAR_START} detail={e}"
            ));
            sidecar.shutdown(Duration::from_secs(10));
            return EXIT_SIDECAR_START;
        }
    };

    let mut sidecar_opt = Some(sidecar);
    let cell = Arc::new(std::sync::Mutex::new(sidecar_opt.take()));
    let cell_for_run = cell.clone();
    let exit_code_for_run = exit_code.clone();
    app.run(move |_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Ok(mut g) = cell_for_run.lock() {
                if let Some(mut s) = g.take() {
                    let reaped = s.shutdown(Duration::from_secs(10));
                    out(&format!(
                        "[desktop] shutdown reason=window-closed reaped={reaped}"
                    ));
                    if !reaped {
                        exit_code_for_run.store(EXIT_SIDECAR_FATAL, Ordering::SeqCst);
                    }
                }
            }
        }
    });
    // 事件循环之外的兜底：若 RunEvent::Exit 未送达，Drop 守卫仍会终止 sidecar，
    // 但这里显式再确认一次并留下可核对的记录。
    if let Ok(mut g) = cell.lock() {
        if let Some(mut s) = g.take() {
            let reaped = s.shutdown(Duration::from_secs(10));
            out(&format!(
                "[desktop] shutdown reason=run-returned reaped={reaped}"
            ));
        }
    }
    out("[desktop] exit");
    exit_code.load(Ordering::SeqCst)
}
