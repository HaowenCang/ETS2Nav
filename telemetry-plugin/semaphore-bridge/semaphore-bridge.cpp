/**
 * @brief ETS2Nav Semaphore Bridge v2（自研游戏内信号灯读取插件）
 *
 * v2 修复：启动卡死问题
 * 1) 延迟扫描：等待 SCS frame_start 事件（游戏世界就绪）后才开始扫描
 * 2) 低优先级：reader 线程 THREAD_PRIORITY_BELOW_NORMAL，不与游戏主线程抢 CPU
 * 3) 重扫节流：timer_restart 或 60 秒间隔；重扫前先快速验证旧基址有效性
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

#define UNUSED(x)

#define NAV_SEM_MAGIC 0x324D4553u
#define NAV_SEM_VERSION 2u
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
    if (time < 0.0f || time > 120.0f) return false;
    int state = *(const int32_t *)(p + 0x28);
    if (!is_valid_state(state)) return false;
    float px = *(const float *)(p + 0x00);
    float pz = *(const float *)(p + 0x08);
    if (px < -500000.0f || px > 500000.0f || pz < -500000.0f || pz > 500000.0f) return false;
    float qx = *(const float *)(p + 0x10), qy = *(const float *)(p + 0x14);
    float qz = *(const float *)(p + 0x18), qw = *(const float *)(p + 0x1C);
    float q2 = qx * qx + qy * qy + qz * qz + qw * qw;
    if (q2 < 0.8f || q2 > 1.2f) return false;
    int16_t cx = *(const int16_t *)(p + 0x0C);
    int16_t cy = *(const int16_t *)(p + 0x0E);
    if (cx < -10000 || cx > 10000 || cy < -10000 || cy > 10000) return false;
    return true;
}

static inline bool slot_hint(const uint8_t *p)
{
    int type = *(const int32_t *)(p + 0x20);
    if (type != 1 && type != 2) return false;
    float time = *(const float *)(p + 0x24);
    if (time < 0.0f || time > 120.0f) return false;
    int state = *(const int32_t *)(p + 0x28);
    return is_valid_state(state);
}

// 扫描地址区间 [start, end)，返回基址或 0
static uintptr_t scan_region(uintptr_t start, uintptr_t end)
{
    uintptr_t addr = start;
    long yieldAt = 0;
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

        const uint8_t *base = (const uint8_t *)regionStart;
        const size_t limit = (size_t)mbi.RegionSize;
        for (size_t i = 0; i + 48 * 2 <= limit; i += 4)
        {
            if (!slot_hint(base + i)) continue;
            if (!slot_matches(base + i)) continue;
            if (slot_matches(base + i + 48) || slot_matches(base + i + 96) || slot_matches(base + i + 144))
                return regionStart + i;
        }
        // 每约 512MB 让出一次调度（低优先级下避免长时间霸占）
        if ((long)regionStart - yieldAt > 512L * 1024 * 1024)
        {
            yieldAt = (long)regionStart;
            Sleep(1);
        }
    }
    return 0;
}

// 顺序扫描全内存（低优先级）
static uintptr_t scan_single(void)
{
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    uintptr_t lo = (uintptr_t)si.lpMinimumApplicationAddress;
    uintptr_t hi = (uintptr_t)si.lpMaximumApplicationAddress;
    if (lo < 0x100000000ull) lo = 0x100000000ull;
    return scan_region(lo, hi);
}

static uintptr_t located_base = 0;
static int located_count = 0;
static LONG last_verify_tick = 0;
static LONG last_full_scan_tick = 0;

static bool verify_base(uintptr_t base)
{
    return base != 0 && slot_matches((const uint8_t *)base);
}

// ---- 读取线程 ----
static HANDLE reader_thread = NULL;
static volatile bool reader_running = false;

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
        LONG now = GetTickCount();

        // 定期验证旧基址有效性（10 秒）
        if (located_base != 0 && now - last_verify_tick > 10000)
        {
            last_verify_tick = now;
            if (!verify_base(located_base)) located_base = 0;
        }

        bool trigger = located_base == 0
            || InterlockedCompareExchange(&need_rescan, 0, 1) == 1
            || (located_base != 0 && now - last_full_scan_tick > 60000);

        if (trigger && located_base == 0)
        {
            uintptr_t found = scan_single();
            if (found)
            {
                located_base = found;
                located_count = 0;
                for (int k = 0; k < NAV_SEM_MAX_LIGHTS; k++)
                {
                    if (slot_matches((const uint8_t *)(found + (size_t)k * 48))) located_count++;
                    else break;
                }
                last_full_scan_tick = now;
                log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore array located 0x%llx (%d lights)",
                    (unsigned long long)found, located_count);
            }
        }
        else if (located_base != 0)
        {
            last_full_scan_tick = now;   // 节流
        }

        if (located_base != 0)
        {
            const uint8_t *src = (const uint8_t *)located_base;
            int n = located_count;
            if (n > NAV_SEM_MAX_LIGHTS) n = NAV_SEM_MAX_LIGHTS;
            memcpy(sem_lights, src, (size_t)n * LIGHT_SIZE);
            *sem_count = (uint32_t)n;
            (*sem_sequence)++;
        }
        Sleep(100);
    }
    return 0;
}

// ---- SCS 事件 ----
SCSAPI_VOID on_frame_start(const scs_event_t UNUSED(event), const void *const event_info, const scs_context_t UNUSED(context))
{
    const struct scs_telemetry_frame_start_t *info = static_cast<const scs_telemetry_frame_start_t *>(event_info);
    // 任何 frame_start 到达即表示游戏仿真运行中（SDK 1.14 无 truck/job 标志，仅 timer_restart）
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

    log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore bridge v2 initialized (lazy scan)");
    return SCS_RESULT_ok;
}

SCSAPI_VOID scs_telemetry_shutdown(void)
{
    reader_running = false;
    if (reader_thread)
    {
        WaitForSingleObject(reader_thread, 3000);
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
