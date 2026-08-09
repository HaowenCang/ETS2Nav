/**
 * @brief ETS2Nav Semaphore Bridge（自研游戏内信号灯读取插件）
 *
 * 目标：零外部依赖的信号灯数据源（不依赖 ETS2LA/视觉检测）。
 *
 * 原理（P0-B 实验结论 + 内存反查验证）：
 * - 游戏内存中存在 48 字节/灯的信号灯状态数组（布局已反查确认）：
 *     +0x00 pos(x,y,z) 3×float
 *     +0x0C cx:short, cy:short（sector 偏移）
 *     +0x10 quat(x,y,z,w) 4×float（单位四元数）
 *     +0x20 type: int（1=信号灯 2=道闸）
 *     +0x24 time_remaining: float（递减 0~60）
 *     +0x28 state: int（1=ORANGE_TO_RED 2=RED 4=ORANGE_TO_GREEN 8=GREEN 32=SLEEP）
 *     +0x2C id: int
 * - 数组基址是堆地址（每次启动变化）→ 启动时特征扫描定位
 *
 * 输出：共享内存 Local\ETS2NavSemaphore
 *   u32 magic "SEM2" + u32 version + u32 sequence + u32 count + count×48B
 *
 * 构建：build.bat（VC 工具链）。放入游戏 plugins 目录。
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

#define NAV_SEM_MAGIC 0x324D4553u      // "SEM2"
#define NAV_SEM_VERSION 1u
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
static uint8_t *sem_mem = NULL;         // 共享内存视图
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

// ---- 信号灯数组定位（特征扫描，进程内） ----

static uintptr_t located_base = 0;
static int located_count = 0;

static inline bool is_valid_state(int s)
{
    return s == 1 || s == 2 || s == 4 || s == 8 || s == 32;
}

// 48 字节槽特征验证（严格模式，用于确认）
static bool slot_matches(const uint8_t *p, bool strict_time)
{
    int type = *(const int32_t *)(p + 0x20);
    if (type != 1 && type != 2) return false;
    float time = *(const float *)(p + 0x24);
    if (time < 0.0f || time > 120.0f) return false;
    int state = *(const int32_t *)(p + 0x28);
    if (!is_valid_state(state)) return false;
    // 位置合理
    float px = *(const float *)(p + 0x00);
    float pz = *(const float *)(p + 0x08);
    if (px < -500000.0f || px > 500000.0f || pz < -500000.0f || pz > 500000.0f) return false;
    // 四元数归一化
    float qx = *(const float *)(p + 0x10), qy = *(const float *)(p + 0x14);
    float qz = *(const float *)(p + 0x18), qw = *(const float *)(p + 0x1C);
    float q2 = qx * qx + qy * qy + qz * qz + qw * qw;
    if (q2 < 0.8f || q2 > 1.2f) return false;
    // cx/cy 合理
    int16_t cx = *(const int16_t *)(p + 0x0C);
    int16_t cy = *(const int16_t *)(p + 0x0E);
    if (cx < -10000 || cx > 10000 || cy < -10000 || cy > 10000) return false;
    (void)strict_time;
    return true;
}

// 宽松模式（用于扫描初筛）：只查 state+time+type
static inline bool slot_hint(const uint8_t *p)
{
    int type = *(const int32_t *)(p + 0x20);
    if (type != 1 && type != 2) return false;
    float time = *(const float *)(p + 0x24);
    if (time < 0.0f || time > 120.0f) return false;
    int state = *(const int32_t *)(p + 0x28);
    return is_valid_state(state);
}

// 扫描进程内存定位信号灯数组
// 返回 true 并设置 located_base / located_count
static bool scan_for_semaphores(void)
{
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    uintptr_t addr = (uintptr_t)si.lpMinimumApplicationAddress;
    const uintptr_t maxAddr = (uintptr_t)si.lpMaximumApplicationAddress;

    while (addr < maxAddr)
    {
        MEMORY_BASIC_INFORMATION mbi;
        if (VirtualQuery((LPCVOID)addr, &mbi, sizeof(mbi)) == 0) break;
        uintptr_t regionStart = addr;
        addr = regionStart + mbi.RegionSize;
        if (mbi.State != MEM_COMMIT) continue;
        if (mbi.Protect == PAGE_NOACCESS || (mbi.Protect & PAGE_GUARD)) continue;
        if (mbi.Protect != PAGE_READWRITE && mbi.Protect != PAGE_READONLY && mbi.Protect != PAGE_WRITECOPY
            && mbi.Protect != PAGE_EXECUTE_READWRITE && mbi.Protect != PAGE_EXECUTE_READ
            && mbi.Protect != (PAGE_READWRITE | PAGE_NOCACHE) && mbi.Protect != (PAGE_READWRITE | PAGE_WRITECOMBINE))
            continue;
        if (regionStart < 0x100000000ull) continue;   // 跳过模块/低地址

        const uint8_t *base = (const uint8_t *)regionStart;
        const size_t limit = (size_t)mbi.RegionSize;
        for (size_t i = 0; i + 48 * 2 <= limit; i += 4)
        {
            if (!slot_hint(base + i)) continue;
            if (!slot_matches(base + i, false)) continue;
            // 连续槽验证（至少 2 个连续有效槽）
            if (slot_matches(base + i + 48, false) || slot_matches(base + i + 96, false) || slot_matches(base + i + 144, false))
            {
                located_base = regionStart + i;
                located_count = 0;
                // 统计连续有效槽数
                for (int k = 0; k < NAV_SEM_MAX_LIGHTS; k++)
                {
                    if (slot_matches(base + i + (size_t)k * 48, false)) located_count++;
                    else break;
                }
                if (located_count >= 2) return true;
            }
        }
    }
    return false;
}

// ---- 读取线程（独立线程，不阻塞游戏主线程） ----

static HANDLE reader_thread = NULL;
static volatile bool reader_running = false;
static volatile LONG last_scan_tick = 0;

static DWORD WINAPI reader_loop(LPVOID)
{
    while (reader_running)
    {
        // 每 3 秒尝试重定位（游戏重载/读档后数组可能重分配）
        LONG now = GetTickCount();
        if (located_base == 0 || now - last_scan_tick > 3000)
        {
            if (scan_for_semaphores())
            {
                last_scan_tick = now;
                log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore array located at 0x%llx (%d lights)",
                    (unsigned long long)located_base, located_count);
            }
            else if (located_base == 0)
            {
                last_scan_tick = now;   // 未找到，3 秒后重试
            }
        }

        if (located_base != 0)
        {
            // 读取到共享内存
            const uint8_t *src = (const uint8_t *)located_base;
            int n = located_count;
            if (n > NAV_SEM_MAX_LIGHTS) n = NAV_SEM_MAX_LIGHTS;
            memcpy(sem_lights, src, (size_t)n * LIGHT_SIZE);
            *sem_count = (uint32_t)n;
            (*sem_sequence)++;
        }
        Sleep(100);   // 10 Hz
    }
    return 0;
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

    reader_running = true;
    reader_thread = CreateThread(NULL, 0, reader_loop, NULL, 0, NULL);
    if (!reader_thread)
    {
        deinit_shared_memory();
        game_log = NULL;
        return SCS_RESULT_generic_error;
    }

    log_line(SCS_LOG_TYPE_message, "ETS2Nav semaphore bridge initialized");
    return SCS_RESULT_ok;
}

SCSAPI_VOID scs_telemetry_shutdown(void)
{
    reader_running = false;
    if (reader_thread)
    {
        WaitForSingleObject(reader_thread, 2000);
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
