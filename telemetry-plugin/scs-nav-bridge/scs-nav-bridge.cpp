/**
 * @brief ETS2Nav Telemetry Bridge（B3）——基于官方 SCS Telemetry SDK 1.14。
 *
 * 职责（v0.2 §6）：仅读取遥测并通过共享内存转发，不执行任何导航逻辑。
 * 布局：Local\ETS2NavTelemetry，含 sequence counter（v0.2 §7）。
 *
 * 基于官方 telemetry_mem 示例骨架（SCS SDK，MIT 许可）扩展：
 * - frame_start 时间戳（simulation/render/paused）
 * - job config 属性（destination/source city/company + ids）
 * - 限速、燃油、休息等通道
 *
 * 构建：build.bat（需 VS 工具链）。输出 scs-nav-bridge.dll 放入游戏 plugins 目录。
 */

#define WINVER 0x0500
#define _WIN32_WINNT 0x0500
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <stdarg.h>
#include <string.h>

#include "scssdk_telemetry.h"
#include "eurotrucks2/scssdk_eut2.h"
#include "eurotrucks2/scssdk_telemetry_eut2.h"
#include "amtrucks/scssdk_ats.h"
#include "amtrucks/scssdk_telemetry_ats.h"

#define UNUSED(x)

/** 共享内存布局版本：布局变更时递增，客户端校验。 */
#define NAV_BRIDGE_LAYOUT_VERSION 1u

scs_telemetry_register_for_channel_t register_for_channel = NULL;
scs_telemetry_unregister_from_channel_t unregister_from_channel = NULL;
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

#pragma pack(push)
#pragma pack(1)

/** 共享内存布局。字段均为定长，客户端按偏移读取。 */
struct telemetry_state_t
{
    // --- 同步 ---
    volatile scs_u32_t sequence;        // 每次帧更新后递增（客户端轮询此值检测新数据）
    scs_u32_t layout_version;           // = NAV_BRIDGE_LAYOUT_VERSION
    scs_u8_t running;                   // telemetry 输出状态（1=运行，0=暂停输出）
    scs_u8_t game_paused;               // 游戏显式暂停（菜单/暂停键）
    scs_u8_t reserved0;
    scs_u8_t reserved1;

    // --- 时间（frame_start 事件，微秒）---
    scs_u64_t simulation_time;
    scs_u64_t paused_simulation_time;
    scs_u64_t render_time;
    scs_u64_t frame_elapsed_ms;         // 连续化时间（处理 timer_restart）
    scs_u32_t game_time_minutes;        // game.time（游戏内分钟，自首个游戏日 00:00）
    scs_float_t local_scale;            // 真实秒 ↔ 游戏秒倍率
    scs_s32_t rest_stop_minutes;        // rest.stop（距强制休息的游戏内分钟，-1=未启用）

    // --- 车辆 ---
    scs_value_dplacement_t placement;   // 世界坐标 + 四元数朝向
    scs_float_t speed;                  // m/s（负值=倒车）
    scs_float_t speed_limit;            // m/s（Route Advisor 值；0=无限速，未文档化行为）
    scs_float_t fuel_amount;            // 升
    scs_float_t fuel_range;             // km
    scs_u8_t fuel_warning;

    // --- 任务（configuration 事件更新，非逐帧）---
    scs_u8_t job_active;                // is_cargo_loaded
    char job_source_city[64];
    char job_source_city_id[64];
    char job_source_company[64];
    char job_source_company_id[64];
    char job_dest_city[64];
    char job_dest_city_id[64];
    char job_dest_company[64];
    char job_dest_company_id[64];
    scs_u32_t job_income;
    scs_u32_t job_delivery_time_minutes;
};

#pragma pack(pop)

static HANDLE shared_memory_handle = NULL;
static struct telemetry_state_t *shared_memory = NULL;

/** 帧起始时间基准（连续化处理 timer_restart，官方 telemetry.cpp 示例方法）。 */
static scs_timestamp_t last_paused_sim_time = static_cast<scs_timestamp_t>(-1);
static scs_u64_t continuous_elapsed_ms = 0;

// ---- 存储回调（回调签名：name, index, value, context）----

static SCSAPI_VOID store_float(const scs_string_t, const scs_u32_t, const scs_value_t *const value, const scs_context_t context)
{
    scs_float_t *const storage = static_cast<scs_float_t *>(context);
    if (storage) *storage = value ? value->value_float.value : 0.0f;
}

static SCSAPI_VOID store_u32(const scs_string_t, const scs_u32_t, const scs_value_t *const value, const scs_context_t context)
{
    scs_u32_t *const storage = static_cast<scs_u32_t *>(context);
    if (storage) *storage = value ? value->value_u32.value : 0;
}

static SCSAPI_VOID store_s32(const scs_string_t, const scs_u32_t, const scs_value_t *const value, const scs_context_t context)
{
    scs_s32_t *const storage = static_cast<scs_s32_t *>(context);
    if (storage) *storage = value ? value->value_s32.value : 0;
}

static SCSAPI_VOID store_bool(const scs_string_t, const scs_u32_t, const scs_value_t *const value, const scs_context_t context)
{
    scs_u8_t *const storage = static_cast<scs_u8_t *>(context);
    if (storage) *storage = value ? (value->value_bool.value ? 1 : 0) : 0;
}

static SCSAPI_VOID store_dplacement(const scs_string_t, const scs_u32_t, const scs_value_t *const value, const scs_context_t context)
{
    scs_value_dplacement_t *const storage = static_cast<scs_value_dplacement_t *>(context);
    if (storage) *storage = value ? value->value_dplacement : scs_value_dplacement_t{};
}

// ---- 事件回调 ----

SCSAPI_VOID telemetry_pause(const scs_event_t event, const void *const UNUSED(event_info), const scs_context_t UNUSED(context))
{
    shared_memory->running = (event == SCS_TELEMETRY_EVENT_started) ? 1 : 0;
    shared_memory->game_paused = (event == SCS_TELEMETRY_EVENT_paused) ? 1 : 0;
}

SCSAPI_VOID telemetry_frame_start(const scs_event_t UNUSED(event), const void *const event_info, const scs_context_t UNUSED(context))
{
    const struct scs_telemetry_frame_start_t *const info = static_cast<const scs_telemetry_frame_start_t *>(event_info);

    shared_memory->simulation_time = info->simulation_time;
    shared_memory->paused_simulation_time = info->paused_simulation_time;
    shared_memory->render_time = info->render_time;

    // 连续化时间：处理 timer_restart（读档/快速旅行后计时器重置）
    if (last_paused_sim_time == static_cast<scs_timestamp_t>(-1)) {
        last_paused_sim_time = info->paused_simulation_time;
    }
    const bool restart = (info->flags & SCS_TELEMETRY_FRAME_START_FLAG_timer_restart) != 0;
    scs_u64_t elapsed = 0;
    if (info->paused_simulation_time >= last_paused_sim_time) {
        elapsed = info->paused_simulation_time - last_paused_sim_time;
    }
    last_paused_sim_time = info->paused_simulation_time;
    continuous_elapsed_ms += restart ? 0 : elapsed / 1000u;
    shared_memory->frame_elapsed_ms = continuous_elapsed_ms;

    // 帧更新完成：递增序号（最后写入，客户端据此判断数据完整）
    shared_memory->sequence++;
}

/** 取 config 属性（类型校验）。 */
static const scs_named_value_t *find_attribute(const scs_telemetry_configuration_t &config, const char *const name, const scs_u32_t index, const scs_value_type_t expected_type)
{
    for (const scs_named_value_t *current = config.attributes; current->name; ++current) {
        if ((index == SCS_U32_NIL || current->index == index) && strcmp(current->name, name) == 0) {
            if (current->value.type == expected_type) {
                return current;
            }
            log_line(SCS_LOG_TYPE_error, "Attribute %s has unexpected type %u", name, static_cast<unsigned>(current->value.type));
            return NULL;
        }
    }
    return NULL;
}

static void copy_string(char *dest, size_t dest_size, const char *src)
{
    if (src) {
        strncpy_s(dest, dest_size, src, _TRUNCATE);
    } else {
        dest[0] = 0;
    }
}

SCSAPI_VOID telemetry_configuration(const scs_event_t event, const void *const event_info, const scs_context_t UNUSED(context))
{
    const struct scs_telemetry_configuration_t *const info = static_cast<const scs_telemetry_configuration_t *>(event_info);

    if (strcmp(info->id, SCS_TELEMETRY_CONFIG_job) == 0) {
        // 任务属性（官方 configs，B1 定稿：destination.city(.id) 系列）
        const scs_named_value_t *v;

        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_is_cargo_loaded, SCS_U32_NIL, SCS_VALUE_TYPE_bool)) != NULL) {
            shared_memory->job_active = v->value.value_bool.value ? 1 : 0;
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_destination_city, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_dest_city, sizeof(shared_memory->job_dest_city), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_destination_city_id, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_dest_city_id, sizeof(shared_memory->job_dest_city_id), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_destination_company, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_dest_company, sizeof(shared_memory->job_dest_company), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_destination_company_id, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_dest_company_id, sizeof(shared_memory->job_dest_company_id), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_source_city, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_source_city, sizeof(shared_memory->job_source_city), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_source_city_id, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_source_city_id, sizeof(shared_memory->job_source_city_id), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_source_company, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_source_company, sizeof(shared_memory->job_source_company), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_source_company_id, SCS_U32_NIL, SCS_VALUE_TYPE_string)) != NULL) {
            copy_string(shared_memory->job_source_company_id, sizeof(shared_memory->job_source_company_id), v->value.value_string.value);
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_income, SCS_U32_NIL, SCS_VALUE_TYPE_u32)) != NULL) {
            shared_memory->job_income = v->value.value_u32.value;
        }
        if ((v = find_attribute(*info, SCS_TELEMETRY_CONFIG_ATTRIBUTE_delivery_time, SCS_U32_NIL, SCS_VALUE_TYPE_u32)) != NULL) {
            shared_memory->job_delivery_time_minutes = v->value.value_u32.value;
        }
        return;
    }

    // 卡车/拖车等其他配置：当前无需处理（扩展点）
}

// ---- 共享内存 ----

static bool initialize_shared_memory(void)
{
    shared_memory_handle = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, sizeof(struct telemetry_state_t), "Local\\ETS2NavTelemetry");
    if (shared_memory_handle == NULL) {
        log_line(SCS_LOG_TYPE_error, "Unable to create shared memory");
        return false;
    }
    shared_memory = static_cast<struct telemetry_state_t *>(MapViewOfFile(shared_memory_handle, FILE_MAP_ALL_ACCESS, 0, 0, sizeof(struct telemetry_state_t)));
    if (shared_memory == NULL) {
        log_line(SCS_LOG_TYPE_error, "Unable to map shared memory");
        CloseHandle(shared_memory_handle);
        shared_memory_handle = NULL;
        return false;
    }
    memset(shared_memory, 0, sizeof(struct telemetry_state_t));
    shared_memory->layout_version = NAV_BRIDGE_LAYOUT_VERSION;
    shared_memory->running = 0;
    shared_memory->game_paused = 1;
    shared_memory->sequence = 0;
    return true;
}

static void deinitialize_shared_memory(void)
{
    if (shared_memory) {
        UnmapViewOfFile(shared_memory);
        shared_memory = NULL;
    }
    if (shared_memory_handle) {
        CloseHandle(shared_memory_handle);
        shared_memory_handle = NULL;
    }
}

// ---- SDK 生命周期 ----

SCSAPI_RESULT scs_telemetry_init(const scs_u32_t version, const scs_telemetry_init_params_t *const params)
{
    if (version != SCS_TELEMETRY_VERSION_1_00) {
        return SCS_RESULT_unsupported;
    }
    const scs_telemetry_init_params_v100_t *const version_params = static_cast<const scs_telemetry_init_params_v100_t *>(params);
    game_log = version_params->common.log;

    log_line(SCS_LOG_TYPE_message, "ETS2Nav bridge initializing (game '%s' %u.%u)", version_params->common.game_id,
             SCS_GET_MAJOR_VERSION(version_params->common.game_version), SCS_GET_MINOR_VERSION(version_params->common.game_version));

    if (strcmp(version_params->common.game_id, SCS_GAME_ID_EUT2) == 0) {
        const scs_u32_t IMPLEMENTED_VERSION = SCS_TELEMETRY_EUT2_GAME_VERSION_CURRENT;
        if (SCS_GET_MAJOR_VERSION(version_params->common.game_version) > SCS_GET_MAJOR_VERSION(IMPLEMENTED_VERSION)) {
            log_line(SCS_LOG_TYPE_warning, "Too new major version of the game, some features might behave incorrectly");
        }
    } else {
        log_line(SCS_LOG_TYPE_warning, "Unsupported game id, some features might behave incorrectly");
    }

    // 事件注册
    const bool events_registered =
        (version_params->register_for_event(SCS_TELEMETRY_EVENT_paused, telemetry_pause, NULL) == SCS_RESULT_ok) &&
        (version_params->register_for_event(SCS_TELEMETRY_EVENT_started, telemetry_pause, NULL) == SCS_RESULT_ok) &&
        (version_params->register_for_event(SCS_TELEMETRY_EVENT_frame_start, telemetry_frame_start, NULL) == SCS_RESULT_ok) &&
        (version_params->register_for_event(SCS_TELEMETRY_EVENT_configuration, telemetry_configuration, NULL) == SCS_RESULT_ok);
    if (!events_registered) {
        log_line(SCS_LOG_TYPE_error, "Unable to register event callbacks");
        game_log = NULL;
        return SCS_RESULT_generic_error;
    }

    if (!initialize_shared_memory()) {
        game_log = NULL;
        return SCS_RESULT_generic_error;
    }

    // 通道注册（官方头文件宏名，B1 定稿）
#define REGISTER_CHANNEL(name, type, field) \
    version_params->register_for_channel(SCS_TELEMETRY_##name, SCS_U32_NIL, SCS_VALUE_TYPE_##type, SCS_TELEMETRY_CHANNEL_FLAG_no_value, store_##type, &shared_memory->field)

    REGISTER_CHANNEL(TRUCK_CHANNEL_world_placement, dplacement, placement);
    REGISTER_CHANNEL(TRUCK_CHANNEL_speed, float, speed);
    REGISTER_CHANNEL(TRUCK_CHANNEL_navigation_speed_limit, float, speed_limit);
    REGISTER_CHANNEL(TRUCK_CHANNEL_fuel, float, fuel_amount);
    REGISTER_CHANNEL(TRUCK_CHANNEL_fuel_range, float, fuel_range);
    REGISTER_CHANNEL(TRUCK_CHANNEL_fuel_warning, bool, fuel_warning);

    REGISTER_CHANNEL(CHANNEL_game_time, u32, game_time_minutes);
    REGISTER_CHANNEL(CHANNEL_local_scale, float, local_scale);
    REGISTER_CHANNEL(CHANNEL_next_rest_stop, s32, rest_stop_minutes);

#undef REGISTER_CHANNEL

    register_for_channel = version_params->register_for_channel;
    unregister_from_channel = version_params->unregister_from_channel;

    log_line(SCS_LOG_TYPE_message, "ETS2Nav bridge initialized");
    return SCS_RESULT_ok;
}

SCSAPI_VOID scs_telemetry_shutdown(void)
{
    deinitialize_shared_memory();
    unregister_from_channel = NULL;
    register_for_channel = NULL;
    game_log = NULL;
}

BOOL APIENTRY DllMain(HMODULE, DWORD, LPVOID)
{
    return TRUE;
}

// EOF //
