// Semaphore profile 模型与继承链解析（B2）
// 基于 docs/format-notes/semaphore-profile.md 的格式结论
// GPL-3.0 — ETS2Nav 项目

using ScsSii;

namespace ScsSii.Semaphore;

/// <summary>单个信号灯相位（一条 interval[] 记录）：绿→黄→红→黄 各段秒数。</summary>
public sealed record SignalPhase(double Green, double Yellow1, double Red, double Yellow2)
{
    public double CycleLength => Green + Yellow1 + Red + Yellow2;
    /// <summary>周期内不可通行（非绿灯）总时长：黄1+红+黄2。</summary>
    public double BlockedLength => Yellow1 + Red + Yellow2;
}

/// <summary>解析后的信号灯 profile（继承链已展开）。</summary>
public sealed class SemaphoreProfile
{
    public required string Name { get; init; }
    /// <summary>各信号灯相位（按下标对应 prefab semaphore ID）。可为空（profile 未定义 interval）。</summary>
    public IReadOnlyList<SignalPhase> Phases { get; init; } = Array.Empty<SignalPhase>();
    public IReadOnlyList<double> Cycle { get; init; } = Array.Empty<double>();
    /// <summary>信号组类型序列（type[]：traffic_light_major/minor 等——P1-10 signal group 类型）。</summary>
    public IReadOnlyList<string> SignalGroupTypes { get; init; } = Array.Empty<string>();
    public double? SleepStart { get; init; }   // 自午夜分钟
    public double? SleepEnd { get; init; }
    /// <summary>夜间闪烁窗口是否启用（且位于窗口内时无周期可言）。</summary>
    public bool HasSleepWindow => SleepStart.HasValue && SleepEnd.HasValue;

    /// <summary>解析所有 interval[] 后的周期总长（各相位按下标对齐；缺项时用 0）。</summary>
    public double? GetCycleLength(int index)
    {
        if (index < 0 || index >= Phases.Count) return null;
        var c = Phases[index].CycleLength;
        return c > 0 ? c : null;
    }
}

public static class SemaphoreProfileResolver
{
    /// <summary>
    /// 从已解析的 SII 文档构建 profile 表，并展开 inherited 继承链。
    /// 规则（格式笔记）：属性未显式赋值时沿用 inherited 链上的值；
    /// interval[]/cycle[] 为数组，子 profile 提供任意数量的元素即覆盖同下标元素，
    /// 其余下标回退到继承源。
    /// </summary>
    public static Dictionary<string, SemaphoreProfile> Resolve(SiiDocument doc)
    {
        var raw = new Dictionary<string, SiiUnit>();
        foreach (var u in doc.Units.Where(u => u.Class == "tr_semaphore_profile"))
            raw[u.Name] = u;

        var resolved = new Dictionary<string, SemaphoreProfile>();
        foreach (var name in raw.Keys)
            ResolveOne(name, raw, resolved, new HashSet<string>());
        return resolved;
    }

    private static SemaphoreProfile ResolveOne(
        string name,
        Dictionary<string, SiiUnit> raw,
        Dictionary<string, SemaphoreProfile> resolved,
        HashSet<string> visiting)
    {
        if (resolved.TryGetValue(name, out var cached)) return cached;
        if (!visiting.Add(name))
            throw new SiiParseException($"semaphore profile 继承循环：{name}");

        var unit = raw[name];

        // 继承链：先解析父级，子级字段覆盖父级
        SemaphoreProfile? parent = null;
        var inherits = unit.Values("inherited").Select(v => v.Str).FirstOrDefault();
        if (inherits != null)
        {
            if (!raw.ContainsKey(inherits))
                throw new SiiParseException($"profile {name} 继承不存在的 {inherits}");
            parent = ResolveOne(inherits, raw, resolved, visiting);
        }

        var intervals = unit.Values("interval[]").ToList();
        var cycles = unit.Values("cycle[]").Select(v => v.Num).ToList();
        var types = unit.Values("type[]").Select(v => v.Str ?? "").ToList();

        // 数组合并：子元素覆盖父同下标，超出部分追加
        var phases = new List<SignalPhase>();
        var cycleOut = new List<double>();
        var typesOut = new List<string>();
        if (parent != null)
        {
            phases.AddRange(parent.Phases);
            cycleOut.AddRange(parent.Cycle);
            typesOut.AddRange(parent.SignalGroupTypes);
        }
        for (int i = 0; i < intervals.Count; i++)
        {
            var t = intervals[i].Tuple!;
            var p = new SignalPhase(t[0], t[1], t[2], t[3]);
            if (i < phases.Count) phases[i] = p; else phases.Add(p);
        }
        for (int i = 0; i < cycles.Count; i++)
        {
            if (i < cycleOut.Count) cycleOut[i] = cycles[i]; else cycleOut.Add(cycles[i]);
        }
        for (int i = 0; i < types.Count; i++)
        {
            if (i < typesOut.Count) typesOut[i] = types[i]; else typesOut.Add(types[i]);
        }

        var sleepStart = unit.Values("sleep_time_start").Select(v => (double?)v.Num).FirstOrDefault()
                         ?? parent?.SleepStart;
        var sleepEnd = unit.Values("sleep_time_end").Select(v => (double?)v.Num).FirstOrDefault()
                       ?? parent?.SleepEnd;

        visiting.Remove(name);
        var result = new SemaphoreProfile
        {
            Name = name,
            Phases = phases,
            Cycle = cycleOut,
            SignalGroupTypes = typesOut,
            SleepStart = sleepStart,
            SleepEnd = sleepEnd,
        };
        resolved[name] = result;
        return result;
    }
}
