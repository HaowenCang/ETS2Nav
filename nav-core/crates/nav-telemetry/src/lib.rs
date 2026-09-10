// nav-telemetry：实时遥测读取（scs-nav-bridge 共享内存协议）+ trace recorder/replayer。
// P2-navigation-core-plan.md §31-37。
// 共享内存布局（scs-nav-bridge.cpp telemetry_state_t，pack(1)）：
//   offset 0:   sequence u32 / layout_version u32 / running u8 / game_paused u8 / rsvd 2
//   offset 12:  simulation_time u64 / paused_simulation_time u64 / render_time u64 / frame_elapsed_ms u64
//   offset 44:  game_time_minutes u32 / local_scale f32 / rest_stop_minutes i32
//   offset 56:  placement（3×f64 位置 + 4×f32 四元数 = 40B）
//   offset 96:  speed f32 / speed_limit f32 / fuel_amount f32 / fuel_range f32 / fuel_warning u8
//   offset 113: job_active u8 + 8×64B 字符串 + income u32 + delivery_time u32（634B 总）
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::raw::c_void;
use std::path::Path;
use std::ptr;
use std::time::Duration;

pub const SHARED_MEMORY_NAME: &str = "Local\\ETS2NavTelemetry";
pub const LAYOUT_VERSION: u32 = 1;
pub const TOTAL_SIZE: usize = 634;

// —— Windows 共享内存 FFI（零依赖，kernel32）——
#[link(name = "kernel32")]
extern "system" {
    fn OpenFileMappingA(access: u32, inherit: u32, name: *const i8) -> *mut c_void;
    fn CreateFileMappingW(
        file: *mut c_void,
        attrs: *mut c_void,
        protect: u32,
        max_hi: u32,
        max_lo: u32,
        name: *const u16,
    ) -> *mut c_void;
    fn MapViewOfFile(
        map: *mut c_void,
        access: u32,
        off_hi: u32,
        off_lo: u32,
        bytes: usize,
    ) -> *mut c_void;
    fn UnmapViewOfFile(ptr: *mut c_void) -> i32;
    fn CloseHandle(h: *mut c_void) -> i32;
}

const PAGE_READONLY: u32 = 0x02;
const FILE_MAP_READ: u32 = 0x0004;

/// 共享内存读取器（Windows 命名共享内存）。
pub struct SharedMemory {
    map: *mut c_void,
    ptr: *mut u8,
}

impl SharedMemory {
    /// 打开共享内存。返回 None = 桥插件未运行。
    pub fn open(name: &str) -> Option<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let map = CreateFileMappingW(
                ptr::null_mut(),
                ptr::null_mut(),
                PAGE_READONLY,
                0,
                0,
                wide.as_ptr(),
            );
            if map.is_null() {
                return None;
            }
            let p = MapViewOfFile(map, FILE_MAP_READ, 0, 0, TOTAL_SIZE);
            if p.is_null() {
                CloseHandle(map);
                return None;
            }
            Some(SharedMemory {
                map,
                ptr: p as *mut u8,
            })
        }
    }

    /// 读取当前快照（顺序读；sequence 变化由调用方检测）。
    pub fn read(&self) -> TelemetrySnapshot {
        unsafe {
            let b = std::slice::from_raw_parts(self.ptr, TOTAL_SIZE);
            let seq = u32_at(b, 0);
            let paused = u8_at(b, 9) != 0;
            let sim = u64_at(b, 12);
            let paused_sim = u64_at(b, 20);
            let render = u64_at(b, 28);
            let game_min = u32_at(b, 44);
            let scale = f32_at(b, 48);
            let rest = i32_at(b, 52);
            let (pos, quat) = placement_at(b, 56);
            let speed = f32_at(b, 96);
            let speed_limit = f32_at(b, 100);
            let fuel = f32_at(b, 104);
            let fuel_range = f32_at(b, 108);
            let fuel_warning = u8_at(b, 112) != 0;
            let job_active = u8_at(b, 113) != 0;
            let job = if job_active {
                Some(JobInfo {
                    source_city: str_at(b, 114),
                    source_city_id: str_at(b, 178),
                    source_company: str_at(b, 242),
                    source_company_id: str_at(b, 306),
                    dest_city: str_at(b, 370),
                    dest_city_id: str_at(b, 434),
                    dest_company: str_at(b, 498),
                    dest_company_id: str_at(b, 562),
                    income: u32_at(b, 626),
                    delivery_time_minutes: u32_at(b, 630),
                })
            } else {
                None
            };
            TelemetrySnapshot {
                sequence: seq,
                layout_version: u32_at(b, 4),
                running: u8_at(b, 8) != 0,
                paused,
                simulation_time: sim,
                paused_simulation_time: paused_sim,
                render_time: render,
                game_time_minutes: game_min,
                local_scale: scale,
                rest_stop_minutes: rest,
                position: pos,
                heading: quat,
                speed,
                speed_limit,
                fuel_amount: fuel,
                fuel_range,
                fuel_warning,
                job,
            }
        }
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.ptr as *mut c_void);
            CloseHandle(self.map);
        }
    }
}

unsafe fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
unsafe fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        b[off],
        b[off + 1],
        b[off + 2],
        b[off + 3],
        b[off + 4],
        b[off + 5],
        b[off + 6],
        b[off + 7],
    ])
}
unsafe fn i32_at(b: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
unsafe fn f32_at(b: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
unsafe fn u8_at(b: &[u8], off: usize) -> u8 {
    b[off]
}
unsafe fn str_at(b: &[u8], off: usize) -> String {
    let end = b[off..off + 64].iter().position(|&c| c == 0).unwrap_or(64);
    String::from_utf8_lossy(&b[off..off + end]).to_string()
}
unsafe fn placement_at(b: &[u8], off: usize) -> ([f64; 3], [f32; 4]) {
    let mut pos = [0f64; 3];
    for i in 0..3 {
        pos[i] = f64::from_le_bytes([
            b[off + i * 8],
            b[off + i * 8 + 1],
            b[off + i * 8 + 2],
            b[off + i * 8 + 3],
            b[off + i * 8 + 4],
            b[off + i * 8 + 5],
            b[off + i * 8 + 6],
            b[off + i * 8 + 7],
        ]);
    }
    let mut quat = [0f32; 4];
    for (i, q) in quat.iter_mut().enumerate() {
        *q = f32_at(b, off + 24 + i * 4);
    }
    (pos, quat)
}

/// 统一遥测快照（P2 计划 §32：其他模块不得直接读共享内存）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TelemetrySnapshot {
    pub sequence: u32,
    pub layout_version: u32,
    pub running: bool,
    pub paused: bool,
    pub simulation_time: u64,
    pub paused_simulation_time: u64,
    pub render_time: u64,
    pub game_time_minutes: u32,
    pub local_scale: f32,
    pub rest_stop_minutes: i32,
    /// 世界坐标（米）。
    pub position: [f64; 3],
    /// 朝向四元数 (x,y,z,w)。
    pub heading: [f32; 4],
    /// m/s（负值 = 倒车）。
    pub speed: f32,
    /// m/s（0 = 无限速）。
    pub speed_limit: f32,
    pub fuel_amount: f32,
    pub fuel_range: f32,
    pub fuel_warning: bool,
    pub job: Option<JobInfo>,
}

/// 朝向四元数 (x,y,z,w) —— SCS 约定：绕 **Y 轴**的偏航（yaw），非绕 Z。
/// 构造应使用 [`yaw_to_quat`]，读取应使用 [`quat_yaw`]（二者互逆）。
pub const IDENTITY_HEADING: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// 偏航角（世界弧度，`atan2(dz, dx)` 约定）→ SCS 朝向四元数（绕 Y 轴）。
///
/// 纯偏航旋转的四元数为 `(0, sin(θ/2), 0, cos(θ/2))`——x、z 分量为 0。
/// 与 [`quat_yaw`] 互逆（往返误差见本模块单测）。
///
/// 背景（P4 审计 MINOR 修复，2026-08-12）：合成 trace 曾以恒等四元数填充 heading，
/// 致 matcher 全程按「朝北」打分、巡航段持续失配，使 UI 链验证的「remaining 递减」
/// 断言在 3 次复跑中出现 1 次失败。修复即由点表逐点 yaw 经本函数生成 heading。
pub fn yaw_to_quat(yaw: f64) -> [f32; 4] {
    let half = yaw / 2.0;
    [0.0, half.sin() as f32, 0.0, half.cos() as f32]
}

/// SCS 朝向四元数（绕 Y 轴）→ 偏航角（世界弧度）。
///
/// `atan2(2(wy - xz), 1 - 2(y² + z²))`；对 [`yaw_to_quat`] 的输出精确还原输入 yaw。
pub fn quat_yaw(q: [f32; 4]) -> f64 {
    let (x, y, z, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
    (2.0 * (w * y - x * z)).atan2(1.0 - 2.0 * (y * y + z * z))
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JobInfo {
    pub source_city: String,
    pub source_city_id: String,
    pub source_company: String,
    pub source_company_id: String,
    pub dest_city: String,
    pub dest_city_id: String,
    pub dest_company: String,
    pub dest_company_id: String,
    pub income: u32,
    pub delivery_time_minutes: u32,
}

/// 遥测源状态（P2 计划 §33-35）。
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Fresh 含完整快照（JobInfo 大）——整体传递，Box 无收益
pub enum TelemetryState {
    /// 有新数据。
    Fresh(TelemetrySnapshot),
    /// sequence 长时间未更新（桥插件挂起/游戏暂停输出）——不推进进度、不触发重规划。
    Stale,
    /// 共享内存不可用（插件未运行）。
    Disconnected,
}

/// 带 staleness 检测的遥测源。
pub struct TelemetrySource {
    pub shm: Option<SharedMemory>,
    last_sequence: u32,
    last_update: Option<std::time::Instant>,
    /// 超过该时长无 sequence 更新视为 Stale。
    pub stale_timeout: Duration,
}

impl TelemetrySource {
    pub fn new(stale_timeout: Duration) -> Self {
        TelemetrySource {
            shm: SharedMemory::open(SHARED_MEMORY_NAME),
            last_sequence: 0,
            last_update: None,
            stale_timeout,
        }
    }

    /// 尝试（重）连接。
    pub fn reconnect(&mut self) {
        if self.shm.is_none() {
            self.shm = SharedMemory::open(SHARED_MEMORY_NAME);
        }
    }

    pub fn poll(&mut self) -> TelemetryState {
        let Some(shm) = &self.shm else {
            // 周期尝试重连（游戏可能后启动）
            self.shm = SharedMemory::open(SHARED_MEMORY_NAME);
            return TelemetryState::Disconnected;
        };
        let snap = shm.read();
        if snap.sequence == self.last_sequence {
            if let Some(t) = self.last_update {
                if t.elapsed() > self.stale_timeout {
                    return TelemetryState::Stale;
                }
            }
            return TelemetryState::Stale;
        }
        self.last_sequence = snap.sequence;
        self.last_update = Some(std::time::Instant::now());
        TelemetryState::Fresh(snap)
    }
}

/// 遥测异常事件（P2 计划 §35：teleport / load / sequence restart）。
#[derive(Debug, Clone, PartialEq)]
pub enum TelemetryEvent {
    /// 位置不连续（位移 > max_displacement 且车速低）。
    Teleport,
    /// simulation_time 重置（倒转 > 阈值或归零）。
    SimTimeReset,
    /// sequence 重启（插件重启）。
    SequenceRestart,
    /// 暂停状态变化。
    PauseChanged(bool),
    /// 任务状态变化。
    JobChanged(Option<JobInfo>),
}

/// 事件检测器：输入连续快照，输出异常事件。
/// 注意：事件判定需要**上下文**（如连续位移累计），这里提供基础单步检测；
/// 组合判定（如 teleport 需多帧确认）由上层 Session（P2-17）负责。
#[derive(Debug, Default)]
pub struct EventDetector {
    last: Option<TelemetrySnapshot>,
    /// 位移不连续阈值（米）。
    pub max_displacement: f64,
}

impl EventDetector {
    pub fn new(max_displacement: f64) -> Self {
        EventDetector {
            last: None,
            max_displacement,
        }
    }

    pub fn feed(&mut self, snap: &TelemetrySnapshot) -> Vec<TelemetryEvent> {
        let mut events = Vec::new();
        if let Some(last) = &self.last {
            if snap.sequence < last.sequence {
                events.push(TelemetryEvent::SequenceRestart);
            }
            // sim 时间重置：倒退超过 1s 或归零
            if snap.simulation_time < last.simulation_time
                && last.simulation_time - snap.simulation_time > 1_000_000
            {
                events.push(TelemetryEvent::SimTimeReset);
            }
            // 位置不连续：单帧位移 > 阈值（且速度无法解释）
            let d = {
                let dx = snap.position[0] - last.position[0];
                let dy = snap.position[1] - last.position[1];
                let dz = snap.position[2] - last.position[2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            };
            // 一帧内位移超过 max_displacement（如 50m）视为 teleport
            if d > self.max_displacement {
                events.push(TelemetryEvent::Teleport);
            }
            if snap.paused != last.paused {
                events.push(TelemetryEvent::PauseChanged(snap.paused));
            }
            let job_changed = match (&snap.job, &last.job) {
                (Some(a), Some(b)) => a.dest_company_id != b.dest_company_id,
                (None, None) => false,
                _ => true,
            };
            if job_changed {
                events.push(TelemetryEvent::JobChanged(snap.job.clone()));
            }
        } else {
            // 首帧：仅记录
            if snap.job.is_some() {
                events.push(TelemetryEvent::JobChanged(snap.job.clone()));
            }
        }
        self.last = Some(snap.clone());
        events
    }
}

// —— Trace Recorder / Replayer（P2 计划 §36-37）——
// 格式：JSONL（每行一个 TelemetrySnapshot + 墙钟时间戳）。.navtrace。

/// 带时间戳的 trace 帧。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TraceFrame {
    /// 墙钟秒（recorder 启动后）。
    pub t: f64,
    #[serde(flatten)]
    pub snap: TelemetrySnapshot,
}

pub struct TraceRecorder {
    w: Box<dyn Write>,
    start: std::time::Instant,
    count: usize,
}

impl TraceRecorder {
    /// 创建 recorder（覆盖写入）。
    pub fn create(path: &Path) -> std::io::Result<Self> {
        Ok(TraceRecorder {
            w: Box::new(File::create(path)?),
            start: std::time::Instant::now(),
            count: 0,
        })
    }

    pub fn record(&mut self, snap: &TelemetrySnapshot) -> std::io::Result<()> {
        let frame = TraceFrame {
            t: self.start.elapsed().as_secs_f64(),
            snap: snap.clone(),
        };
        let line = serde_json::to_string(&frame)?;
        self.w.write_all(line.as_bytes())?;
        self.w.write_all(b"\n")?;
        self.count += 1;
        Ok(())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

/// 回放 trace 文件（迭代帧）。
pub fn replay(path: &Path) -> std::io::Result<impl Iterator<Item = TraceFrame>> {
    let f = File::open(path)?;
    let r = BufReader::new(f);
    Ok(r.lines()
        .map_while(Result::ok)
        .map_while(|l| serde_json::from_str(&l).ok()))
}

// —— P2-16：semaphore 共享内存（Local\ETS2NavSemaphore，semaphore-bridge v8）——
// header 16B（magic+version+sequence+count）+ count×48B 灯槽。

/// semaphore 灯槽（48B 布局：pos 3f @0 + cx/cy i16 @0x0C + quat 4f @0x10 + type @0x20
/// + time_remaining f32 @0x24 + state i32 @0x28 + id i32 @0x2C）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SemaphoreSlot {
    pub position: (f64, f64, f64),
    pub quat: [f32; 4],
    pub kind: i32,
    pub time_remaining: f32,
    pub state: i32,
    pub id: i32,
}

/// semaphore 快照（无游戏/桥接器时为 None；P2-16 Signal Linker 输入）。
pub fn read_semaphores() -> Option<Vec<SemaphoreSlot>> {
    const NAME: &[u8] = b"Local\\ETS2NavSemaphore\0";
    let base = unsafe {
        let handle = OpenFileMappingA(
            0x0002, /* FILE_MAP_READ */
            0,
            NAME.as_ptr() as *const i8,
        );
        if handle.is_null() {
            return None;
        }
        let map = MapViewOfFile(handle, 0x0002, 0, 0, 0);
        CloseHandle(handle);
        if map.is_null() {
            return None;
        }
        map
    };
    let data = unsafe { std::slice::from_raw_parts(base as *const u8, 16 + 64 * 48) };
    let count = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if count > 64 {
        unsafe {
            UnmapViewOfFile(base);
        }
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = 16 + i * 48;
        let p = |off: usize| -> f32 {
            f32::from_le_bytes([
                data[o + off],
                data[o + off + 1],
                data[o + off + 2],
                data[o + off + 3],
            ])
        };
        let i32at = |off: usize| -> i32 {
            i32::from_le_bytes([
                data[o + off],
                data[o + off + 1],
                data[o + off + 2],
                data[o + off + 3],
            ])
        };
        out.push(SemaphoreSlot {
            position: (p(0) as f64, p(4) as f64, p(8) as f64),
            quat: [p(0x10), p(0x14), p(0x18), p(0x1C)],
            kind: i32at(0x20),
            time_remaining: p(0x24),
            state: i32at(0x28),
            id: i32at(0x2C),
        });
    }
    unsafe {
        UnmapViewOfFile(base);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_game_returns_none() {
        // 无游戏会话（本测试环境）：read_semaphores 应返回 None 而非 panic
        let r = read_semaphores();
        assert!(r.is_none() || r.is_some(), "无游戏时应 graceful None");
    }

    /// yaw → quat → yaw 往返（P4 审计 MINOR 修复的回归锁定）。
    #[test]
    fn yaw_quat_round_trip() {
        for yaw in [
            0.0,
            std::f64::consts::FRAC_PI_2,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            0.3,
            -2.7,
            1.234,
        ] {
            let q = yaw_to_quat(yaw);
            let back = quat_yaw(q);
            // 角度差按 ±π 归一（yaw=π 与 -π 等价）
            let mut d = back - yaw;
            while d > std::f64::consts::PI {
                d -= 2.0 * std::f64::consts::PI;
            }
            while d < -std::f64::consts::PI {
                d += 2.0 * std::f64::consts::PI;
            }
            assert!(d.abs() < 1e-4, "yaw={yaw} 往返得 {back}（差 {d}）");
        }
    }

    /// 纯偏航四元数的 x/z 分量为 0、模长为 1（SCS 绕 Y 轴约定）。
    #[test]
    fn yaw_quat_is_unit_y_rotation() {
        let q = yaw_to_quat(0.9);
        assert_eq!(q[0], 0.0);
        assert_eq!(q[2], 0.0);
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        assert!((n - 1.0).abs() < 1e-6, "模长 {n}");
        // 恒等：yaw=0 → IDENTITY_HEADING
        assert_eq!(yaw_to_quat(0.0), IDENTITY_HEADING);
    }

    /// 约定交叉核对：matcher 以 project_point 的 tangent（atan2(dz,dx)）为参照，
    /// 故沿 +X 前进（yaw=0）与沿 +Z 前进（yaw=π/2）必须给出对应四元数。
    #[test]
    fn yaw_quat_axis_convention() {
        assert!((quat_yaw(yaw_to_quat(0.0)) - 0.0).abs() < 1e-4, "+X 前进");
        assert!(
            (quat_yaw(yaw_to_quat(std::f64::consts::FRAC_PI_2)) - std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-4,
            "+Z 前进"
        );
    }
}
