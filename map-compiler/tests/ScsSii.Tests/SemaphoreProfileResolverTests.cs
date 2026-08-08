using ScsSii;
using ScsSii.Semaphore;

namespace ScsSii.Tests;

public class SemaphoreProfileResolverTests
{
    private static Dictionary<string, SemaphoreProfile> Resolve(string sii) =>
        SemaphoreProfileResolver.Resolve(SiiParser.Parse(sii));

    [Fact]
    public void ResolvesSimpleProfile()
    {
        var map = Resolve("""
            SiiNunit
            {
            tr_semaphore_profile : tr_sem_prof.2ph
            {
            	name: "two phases"
            	interval[]: (15.0, 2.0, 23.0, 2.0)
            	cycle[]: 0.0
            	cycle[]: 21.0
            }
            }
            """);
        var p = map["tr_sem_prof.2ph"];
        // 2ph：1 条 interval（两相位共用间隔，用 cycle[] 偏移错开），2 个 cycle（每灯一个偏移）
        Assert.Equal(1, p.Phases.Count);
        Assert.Equal(new SignalPhase(15, 2, 23, 2), p.Phases[0]);
        Assert.Equal(42.0, p.GetCycleLength(0));
        Assert.Equal(2, p.Cycle.Count);
        Assert.Equal(21.0, p.Cycle[1]);
        Assert.False(p.HasSleepWindow);
    }

    [Fact]
    public void InheritedChainMergesArrays()
    {
        var map = Resolve("""
            SiiNunit
            {
            tr_semaphore_profile : base.profile
            {
            	interval[]: (10.0, 1.0, 15.0, 1.0)
            	interval[]: (8.0, 1.0, 12.0, 1.0)
            	cycle[]: 0.0
            	sleep_time_start: 1410
            	sleep_time_end: 180
            }
            tr_semaphore_profile : child.profile
            {
            	inherited: base.profile
            	interval[]: (20.0, 2.0, 30.0, 2.0)
            	cycle[]: 5.0
            }
            }
            """);
        var child = map["child.profile"];
        // 子 profile 覆盖下标 0，下标 1 回退父级
        Assert.Equal(new SignalPhase(20, 2, 30, 2), child.Phases[0]);
        Assert.Equal(new SignalPhase(8, 1, 12, 1), child.Phases[1]);
        // cycle 覆盖，sleep 继承
        Assert.Equal(5.0, child.Cycle[0]);
        Assert.Equal(1410.0, child.SleepStart);
        Assert.Equal(180.0, child.SleepEnd);
        Assert.True(child.HasSleepWindow);
    }

    [Fact]
    public void DetectsInheritanceCycle()
    {
        Assert.Throws<SiiParseException>(() => Resolve("""
            SiiNunit
            {
            tr_semaphore_profile : a.profile
            {
            	inherited: b.profile
            }
            tr_semaphore_profile : b.profile
            {
            	inherited: a.profile
            }
            }
            """));
    }

    [Fact]
    public void DetectsMissingInheritedTarget()
    {
        Assert.Throws<SiiParseException>(() => Resolve("""
            SiiNunit
            {
            tr_semaphore_profile : a.profile
            {
            	inherited: ghost.profile
            }
            }
            """));
    }

    [Fact]
    public void ComputesBlockedLengthForCostModel()
    {
        var map = Resolve("""
            SiiNunit
            {
            tr_semaphore_profile : p
            {
            	interval[]: (15.0, 2.0, 23.0, 2.0)
            }
            }
            """);
        var p = map["p"];
        Assert.Equal(42.0, p.Phases[0].CycleLength);
        Assert.Equal(27.0, p.Phases[0].BlockedLength);
    }
}
