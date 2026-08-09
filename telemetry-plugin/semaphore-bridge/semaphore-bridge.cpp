/**
 * @brief ETS2Nav Semaphore Bridge v3（自研游戏内信号灯读取插件）
 *
 * v2 修复：启动卡死（延迟扫描 + 低优先级 + 重扫节流）
 * v3 修复：误定位——加强槽验证（time≤60、pos 非零、cx/cy≠-1、连续≥2槽）+ 候选列表
 *
 * 数据布局（内存反查确认，48 字节/灯）：
 *   +0x00 pos(x,y,z) +0x0C cx/cy(short) +0x10 quat(4f) +0x20 type(int)
 *   +0x24 time_remaining(float) +0x28 state(int) +0x2C id(int)
 * 输出：Local\ETS2NavSemaphore = magic+version+sequence+count+count×48B
 */

#define WINVER 0x0500
#define _WIN32_WINNT 0x0500
#include <windows.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>

#include "scssdk_telemetry.h"
#include "eurotrucks2/scssdk_eut2.h"
#include "eurotrucks2/scssdk_telemetry_eut2.h"
#include "amtrucks/scssdk_ats.h"
#include "amtrucks/scssdk_telemetry_ats.h"

#include <math.h>

#define UNUSED(x)

#define NAV_SEM_MAGIC 0x324D4553u
#define NAV_SEM_VERSION 3u
#define NAV_SEM_MAX_LIGHTS 64u
#define LIGHT_SIZE 48u

scs_log_t game_log = NULL;

static void log_line(const scs_log_type_t type, const char *const text, ...)
{
    if (!game_log) return;
    char formatted[1000];
    va_list args;
    va_start(args, text);
    vsnprintf_s(formatted, sizeof(formatted), _TRUNCATE, text, args);
    formatted[sizeof(formatted) - 1] = 0;
    va_end(args);
    game_log(type, formatted);
}

// ---- 共享内存 ----
static HANDLE sem_mem_handle = NULL;
static uint8_t *sem_mem = NULL;
static volatile uint32_t *sem_sequence = NULL;
static uint32_t *sem_count = NULL;
static uint8_t *sem_lights = NULL;

static bool init_shared_memory(void)
{
    const size_t size = 16 + NAV_SEM_MAX_LIGHTS * LIGHT_SIZE;
    sem_mem_handle = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, (DWORD)size, "Local\\ETS2NavSemaphore");
    if (!sem_mem_handle) return false;
    sem_mem = (uint8_t *)MapViewOfFile(sem_mem_handle, FILE_MAP_ALL_ACCESS, 0, 0, size);
    if (!sem_mem) { CloseHandle(sem_mem_handle); sem_mem_handle = NULL; return false; }
    memset(sem_mem, 0, size);
    *(uint32_t *)(sem_mem + 0) = NAV_SEM_MAGIC;
    *(uint32_t *)(sem_mem + 4) = NAV_SEM_VERSION;
    sem_sequence = (volatile uint32_t *)(sem_mem + 8);
    sem_count = (uint32_t *)(sem_mem + 12);
    sem_lights = sem_mem + 16;
    return true;
}

static void deinit_shared_memory(void)
{
    if (sem_mem) { UnmapViewOfFile(sem_mem); sem_mem = NULL; }
    if (sem_mem_handle) { CloseHandle(sem_mem_handle); sem_mem_handle = NULL; }
}

// ---- 状态标志 ----
static volatile LONG game_ready = 0;
static volatile LONG need_rescan = 0;

// ---- 特征验证 ----
static inline bool is_valid_state(int s)
{
    return s == 1 || s == 2 || s == 4 || s == 8 || s == 32;
}

static bool slot_matches(const uint8_t *p)
{
    int type = *(const int32_t *)(p + 0x20);
    if (type != 1 && type != 2) return false;
    float time = *(const float *)(p + 0x24);
    if (!(time >= 0.0f && time <= 60.0f)) return false;   // NaN 穿透防护
    int state = *(const int32_t *)(p + 0x28);
    if (!is_valid_state(state)) return false;
    float px = *(const float *)(p + 0x00);
    float py = *(const float *)(p + 0x04);
    float pz = *(const float *)(p + 0x08);
    // NaN/Infinity 显式排除（NaN 比较全 false，会穿透范围检查）
    if (!(px == px && py == py && pz == pz)) return false;
    if (!(px > -500000.0f && px < 500000.0f && pz > -500000.0f && pz < 500000.0f)) return false;
    if (!(px * px + pz * pz >= 1.0f)) return false;   // pos 非零
    float qx = *(const float *)(p + 0x10), qy = *(const float *)(p + 0x14);
    float qz = *(const float *)(p + 0x18), qw = *(const float *)(p + 0x1C);
    if (!(qx == qx && qy == qy && qz == qz && qw == qw)) return false;
    float q2 = qx * qx + qy * qy + qz * qz + qw * qw;
    if (!(q2 >= 0.8f && q2 <= 1.2f)) return false;
    int16_t cx = *(const int16_t *)(p + 0x0C);
    int16_t cy = *(const int16_t *)(p + 0x0E);
    if (cx < -10000 || cx > 10000 || cy < -10000 || cy > 10000) return false;
    if (cx == -1 && cy == -1) return false;   // 未初始化槽
    return true;
}

static inline bool slot_hint(const uint8_t *p)
{
    int type = *(const int32_t *)(p + 0x20);
    if (type != 1 && type != 2) return false;
    float time = *(const float *)(p + 0x24);
    if (time < 0.0f || time > 60.0f) return false;
    int state = *(const int32_t *)(p + 0x28);
    return is_valid_state(state);
}

// 前向声明（scan_region 协作取消用）
static volatile bool reader_running = false;

// 扫描地址区间，收集候选（最多 outMax 个）
static int scan_region(uintptr_t start, uintptr_t end, uintptr_t *out, int outMax)
{
    int found = 0;
    uintptr_t addr = start;
    uintptr_t yieldAt = 0;
    while (addr < end)
    {
        MEMORY_BASIC_INFORMATION mbi;
        if (VirtualQuery((LPCVOID)addr, &mbi, sizeof(mbi)) == 0) break;
        uintptr_t regionStart = addr;
        addr = regionStart + mbi.RegionSize;
        if (mbi.State != MEM_COMMIT) continue;
        DWORD prot = mbi.Protect;
        if (prot == PAGE_NOACCESS || (prot & PAGE_GUARD)) continue;
        if (prot != PAGE_READWRITE && prot != PAGE_READONLY && prot != PAGE_WRITECOPY
            && prot != PAGE_EXECUTE_READWRITE && prot != PAGE_EXECUTE_READ
            && prot != (PAGE_READWRITE | PAGE_NOCACHE) && prot != (PAGE_READWRITE | PAGE_WRITECOMBINE))
            continue;
        if (regionStart < 0x100000000ull) continue;
        // 跳过自身共享内存映射区（防自定位）
        if (sem_mem != NULL && regionStart >= (uintptr_t)sem_mem && regionStart < (uintptr_t)sem_mem + 16 + NAV_SEM_MAX_LIGHTS * LIGHT_SIZE)
            continue;
        if (regionStart < (uintptr_t)sem_mem && (uintptr_t)sem_mem < regionStart + mbi.RegionSize)
            continue;

        const uint8_t *base = (const uint8_t *)regionStart;
        const size_t limit = (size_t)mbi.RegionSize;
        // i + 192 <= limit 保证 slot_matches(i+144) 读取 [i+144, i+192) 不越界
        for (size_t i = 0; i + 48 * 4 <= limit; i += 4)
        {
            if (!slot_hint(base + i)) continue;
            if (!slot_matches(base + i)) continue;
            if (slot_matches(base + i + 48) || slot_matches(base + i + 96) || slot_matches(base + i + 144))
            {
                out[found++] = regionStart + i;
                if (found >= outMax) return found;
            }
            // 协作取消点 + yield（修复：64 位地址截断 + 无取消点）
            if ((regionStart + i - yieldAt) > 256ull * 1024 * 1024)
            {
                yieldAt = regionStart + i;
                if (!reader_running) return found;   // 卸载时快速退出（BLOCKER-1）
                Sleep(1);
            }
        }
    }
    return found;
}

// 顺序扫描全内存（低优先级），返回候选列表
static int scan_single(uintptr_t *out, int outMax)
{
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    uintptr_t lo = (uintptr_t)si.lpMinimumApplicationAddress;
    uintptr_t hi = (uintptr_t)si.lpMaximumApplicationAddress;
    if (lo < 0x100000000ull) lo = 0x100000000ull;
    return scan_region(lo, hi, out, outMax);
}

static uintptr_t located_base = 0;
static int located_count = 0;
static LONG last_verify_tick = 0;
static LONG last_full_scan_tick = 0;

static bool verify_base(uintptr_t base)
{
    return base != 0 && slot_matches((const uint8_t *)base);
}

// 统计数组跨度：从基址到最后一个有效槽（容忍间隙，连续 4 空槽截断）
// 同时返回有效槽总数（客户端可用 count 过滤空槽）
static int array_span(uintptr_t base, int *validOut)
{
    int span = 0;
    int valid = 0;
    int emptyRun = 0;
    for (int k = 0; k < NAV_SEM_MAX_LIGHTS; k++)
    {
        if (slot_matches((const uint8_t *)(base + (size_t)k * 48)))
        {
            span = k + 1;
            valid++;
            emptyRun = 0;
        }
        else
        {
            emptyRun++;
            if (emptyRun >= 4) break;   // 连续 4 空槽：数组结束
        }
    }
    if (validOut) *validOut = valid;
    return span;
}

// ---- 读取线程 ----
static HANDLE reader_thread = NULL;


static DWORD WINAPI reader_loop(LPVOID)
{
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    // 等待游戏世界就绪（最多 120 秒）
    for (int i = 0; i < 1200 && reader_running; i++)
    {
        if (InterlockedCompareExchange(&game_ready, 0, 1)) break;
        Sleep(100);
    }
    if (!reader_running) return 0;
    log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore: game ready, starting scan");

    while (reader_running)
    {
        __try
        {
            LONG now = GetTickCount();

        if (located_base != 0 && now - last_verify_tick > 1000)
        {
            last_verify_tick = now;
            if (!verify_base(located_base)) located_base = 0;
        }

        bool trigger = located_base == 0
            || InterlockedCompareExchange(&need_rescan, 0, 1) == 1
            || (located_base != 0 && now - last_full_scan_tick > 60000);

        if (trigger && located_base == 0)
        {
            uintptr_t cands[16];
            int nc = scan_single(cands, 16);
            if (nc > 0)
            {
                for (int ci = 0; ci < nc; ci++)
                {
                    int valid = 0;
                    int span = array_span(cands[ci], &valid);
                    if (valid >= 4)   // 至少 4 个有效槽（弱候选通常只有 1-3 个）
                    {
                        located_base = cands[ci];
                        located_count = span;   // 跨度（含间隙槽），客户端过滤
                        last_full_scan_tick = now;
                        log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore array located 0x%llx (span %d, %d valid)",
                            (unsigned long long)cands[ci], span, valid);
                        break;
                    }
                }
            }
        }
        else if (located_base != 0)
        {
            last_full_scan_tick = now;
        }

        if (located_base != 0)
        {
            const uint8_t *src = (const uint8_t *)located_base;
            int n = located_count;
            if (n > NAV_SEM_MAX_LIGHTS) n = NAV_SEM_MAX_LIGHTS;
            // SEH 保护：数组可能被游戏重分配（读档/区域切换），访问违例时置零重扫
            __try
            {
                memcpy(sem_lights, src, (size_t)n * LIGHT_SIZE);
                *sem_count = (uint32_t)n;
                (*sem_sequence)++;
            }
            __except (EXCEPTION_EXECUTE_HANDLER)
            {
                located_base = 0;   // 数组失效 → 重扫
                log_line(SCS_LOG_TYPE_warning, "ETS2Nav semaphore: array access fault, rescanning");
            }
        }
        Sleep(100);
        }
        __except (EXCEPTION_EXECUTE_HANDLER)
        {
            // 兜底：任何访问违例（数组重分配/页面回收）→ 置零重扫，绝不崩溃
            located_base = 0;
            log_line(SCS_LOG_TYPE_warning, "ETS2Nav semaphore: access fault caught, rescanning");
            Sleep(200);
        }
    }
    return 0;
}

// ---- SCS 事件 ----
SCSAPI_VOID on_frame_start(const scs_event_t UNUSED(event), const void *const event_info, const scs_context_t UNUSED(context))
{
    const struct scs_telemetry_frame_start_t *info = static_cast<const scs_telemetry_frame_start_t *>(event_info);
    InterlockedExchange(&game_ready, 1);
    if (info->flags & SCS_TELEMETRY_FRAME_START_FLAG_timer_restart)
        InterlockedExchange(&need_rescan, 1);
}

// ---- SDK 生命周期 ----
SCSAPI_RESULT scs_telemetry_init(const scs_u32_t version, const scs_telemetry_init_params_t *const params)
{
    if (version != SCS_TELEMETRY_VERSION_1_00) return SCS_RESULT_unsupported;
    const scs_telemetry_init_params_v100_t *const vp = static_cast<const scs_telemetry_init_params_v100_t *>(params);
    game_log = vp->common.log;

    if (!init_shared_memory())
    {
        log_line(SCS_LOG_TYPE_error, "ETS2Nav semaphore: shared memory init failed");
        game_log = NULL;
        return SCS_RESULT_generic_error;
    }

    if (vp->register_for_event(SCS_TELEMETRY_EVENT_frame_start, on_frame_start, NULL) != SCS_RESULT_ok)
        log_line(SCS_LOG_TYPE_warning, "ETS2Nav semaphore: frame_start registration failed");

    reader_running = true;
    reader_thread = CreateThread(NULL, 0, reader_loop, NULL, 0, NULL);
    if (!reader_thread)
    {
        deinit_shared_memory();
        game_log = NULL;
        return SCS_RESULT_generic_error;
    }
    SetThreadPriority(reader_thread, THREAD_PRIORITY_BELOW_NORMAL);

    log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore bridge v3 initialized (lazy scan)");
    return SCS_RESULT_ok;
}

SCSAPI_VOID scs_telemetry_shutdown(void)
{
    reader_running = false;
    if (reader_thread)
    {
        // 协作取消后等待线程退出（扫描循环已含取消点，退出 ≤ 数秒）；超时兜底继续等待而非直接 CloseHandle
        for (int i = 0; i < 100; i++)
        {
            if (WaitForSingleObject(reader_thread, 100) == WAIT_OBJECT_0) break;
        }
        CloseHandle(reader_thread);
        reader_thread = NULL;
    }
    deinit_shared_memory();
    game_log = NULL;
}

BOOL APIENTRY DllMain(HMODULE, DWORD, LPVOID)
{
    return TRUE;
}

// EOF //
