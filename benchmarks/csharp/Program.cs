// The benchmark's trees on bt-tree (https://github.com/suilevap/bt-tree), the
// C# library FlatBT descends from. World, leaves and trees are a line-for-line
// port of ../src/common.rs and ../src/soldier.rs; the checksum proves it.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using BT;

enum Outcome { Success, Failure, Running }

sealed class Soldier
{
    public int Health = 100;
    public byte Medkits = 2, Mag = 8, Grenades = 2, Food = 1;
    public ushort Reserve = 24;
    public uint Hunger, Fatigue, FoeLeft, Kills, Deaths;
    public int FoeHp, FoeDist;
    public bool FoeGrouped;
    public int? Noise;
    public byte[] Work = new byte[9];
    public uint[] Effects = new uint[14];

    public bool Foe => FoeLeft > 0;

    public void HurtFoe(int damage, Bb bb)
    {
        FoeHp -= damage;
        if (FoeHp <= 0)
        {
            FoeLeft = 0;
            Kills += 1;
            bb.Score += 100;
        }
    }

    public void Step(uint h)
    {
        Hunger += 1;
        Fatigue += 1;
        if (Foe)
        {
            FoeLeft -= 1;
            if (h % 3 == 0) Health -= 4;
            if (h % 7 == 0 && FoeDist > 0) FoeDist -= 1;
        }
        else if (h % 89 == 0)
        {
            FoeLeft = 60 + h % 60;
            FoeHp = 3 + (int)(h % 4);
            FoeDist = 15 + (int)(h % 15);
            FoeGrouped = h % 2 == 0;
        }
        if (h % 61 == 0) Noise = (int)(h % 31) - 15;
    }
}

sealed class Bb : IBlackboard
{
    public uint T;
    public byte Mode;
    public bool Enemy;
    public int Hp = 100, Pos;
    public uint Charge;
    public ulong Score;
    public readonly bool Rich;
    public Soldier Soldier = new Soldier();

    public Bb(string scenario, uint t)
    {
        T = t;
        Rich = scenario == "soldier";
    }

    public void StepWorld()
    {
        unchecked
        {
            T += 1;
            uint h = T * 2654435761u >> 13;
            Mode = (byte)(h % 8);
            Enemy = h % 5 == 0;
            if (Enemy) Hp -= 9;
            if (Rich) Soldier.Step(h);
        }
    }

    public void UpdateBt(TimeSpan time) { }
    public void Reset() { }
    public void RunningActionChanged<T>(Node<T> runningNode, Path<T> path) where T : IBlackboard { }
}

/// A multi-tick leaf through bt-tree's documented extension point. Start
/// always succeeds; IsInProgress runs the step, Complete reports its result.
/// The node is shared by every agent; `_last` lives only within one update.
sealed class OpAction : ActionNode<Bb>
{
    private readonly Func<Bb, Outcome> _op;
    private Outcome _last;

    public OpAction(string name, Func<Bb, Outcome> op) : base(name) { _op = op; }

    protected internal override bool Start(Bb bb, NodeContext<Bb> c) => true;

    protected internal override bool IsInProgress(Bb bb, NodeContext<Bb> c)
    {
        _last = _op(bb);
        return _last == Outcome.Running;
    }

    protected internal override void Tick(Bb bb, NodeContext<Bb> c) { }

    protected internal override bool Complete(Bb bb, NodeContext<Bb> c) => _last == Outcome.Success;
}

static class Trees
{
    static readonly BTBuilder<Bb> bt = BTBuilder<Bb>.Instance;

    static Node<Bb> Cond(Func<Bb, bool> f) => bt.Condition("cond", f);
    static Node<Bb> Instant(Action<Bb> f) => bt.Action("act", (bb, c) => { f(bb); return true; });
    static Node<Bb> Op(Func<Bb, Outcome> f) => new OpAction("op", f);

    static Node<Bb> Act(uint n) => Instant(bb => bb.Score += n);

    static Node<Bb> MoveTo(int x) => Op(bb =>
    {
        if (bb.Pos == x) return Outcome.Success;
        bb.Pos += Math.Sign(x - bb.Pos);
        return Outcome.Running;
    });

    static Node<Bb> Patrol() => bt.Sequence("patrol", MoveTo(8), Act(1), MoveTo(-8), Act(2));

    public static Node<Bb> Build(string scenario) => scenario switch
    {
        "select8" => bt.Selector("select8", Enumerable.Range(0, 8)
            .Select(i => bt.Sequence("branch", Cond(bb => bb.Mode == i), Act((uint)i)))
            .ToArray()),
        "patrol" => Patrol(),
        "guard" => bt.Selector("guard",
            bt.Sequence("flee", Cond(bb => bb.Hp < 30), MoveTo(0), Instant(bb => { bb.Hp = 100; bb.Score += 1000; })),
            bt.Sequence("fight", Cond(bb => bb.Enemy), Op(bb =>
            {
                bb.Charge += 1;
                if (bb.Charge < 3) return Outcome.Running;
                bb.Charge = 0;
                bb.Score += 100;
                return Outcome.Success;
            })),
            Patrol()),
        "soldier" => SoldierTree.Build(),
        _ => throw new ArgumentException(scenario),
    };

    static class SoldierTree
    {
        enum Abort { Never, OnFoe, OnFoeGone }
        enum Effect { Idle, Respawn, UseMedkit, Heal, Reload, Melee, TakeAmmo, Throw, Fire, ClearNoise, Eat, TakeFood, Sleep, Score }

        static Node<Bb> C(Func<Soldier, bool> f) => Cond(bb => f(bb.Soldier));

        static Node<Bb> Go(Func<Bb, int> target, bool yields) => Op(bb =>
        {
            if (yields && bb.Soldier.Foe) return Outcome.Failure;
            int to = target(bb);
            if (bb.Pos == to) return Outcome.Success;
            bb.Pos += Math.Sign(to - bb.Pos);
            return Outcome.Running;
        });

        static Node<Bb> Go(int at, bool yields) => Go(_ => at, yields);

        static Node<Bb> Work(int slot, byte ticks, Abort abort, Effect effect, int arg = 0) => Op(bb =>
        {
            var s = bb.Soldier;
            bool stop = abort switch { Abort.OnFoe => s.Foe, Abort.OnFoeGone => !s.Foe, _ => false };
            if (stop)
            {
                s.Work[slot] = 0;
                return Outcome.Failure;
            }
            s.Work[slot] += 1;
            if (s.Work[slot] < ticks) return Outcome.Running;
            s.Work[slot] = 0;
            Apply(effect, arg, bb);
            return Outcome.Success;
        });

        static Node<Bb> Do(Effect effect, int arg = 0) => Instant(bb => Apply(effect, arg, bb));

        static Node<Bb> Close(int range) => Op(bb =>
        {
            var s = bb.Soldier;
            if (!s.Foe) return Outcome.Failure;
            if (s.FoeDist <= range) return Outcome.Success;
            s.FoeDist -= 1;
            return Outcome.Running;
        });

        static void Apply(Effect effect, int arg, Bb bb)
        {
            var s = bb.Soldier;
            s.Effects[(int)effect] += 1;
            switch (effect)
            {
                case Effect.Idle: break;
                case Effect.Respawn:
                    bb.Soldier = new Soldier
                    {
                        Deaths = s.Deaths + 1,
                        Kills = s.Kills,
                        Effects = s.Effects,
                        Hunger = s.Hunger,
                        Fatigue = s.Fatigue,
                        FoeLeft = s.FoeLeft,
                        FoeHp = s.FoeHp,
                        FoeDist = s.FoeDist + 10,
                        FoeGrouped = s.FoeGrouped,
                    };
                    bb.Pos = -20;
                    break;
                case Effect.UseMedkit:
                    s.Medkits -= 1;
                    s.Health = Math.Min(s.Health + 50, 100);
                    break;
                case Effect.Heal: s.Health = Math.Min(s.Health + arg, 100); break;
                case Effect.Reload:
                    var take = (ushort)Math.Min(8 - s.Mag, s.Reserve);
                    s.Reserve -= take;
                    s.Mag += (byte)take;
                    break;
                case Effect.Melee: s.HurtFoe(2, bb); break;
                case Effect.TakeAmmo: s.Reserve += 16; break;
                case Effect.Throw:
                    s.Grenades -= 1;
                    s.HurtFoe(10, bb);
                    bb.Score += 200;
                    break;
                case Effect.Fire:
                    s.Mag -= 1;
                    s.HurtFoe(1, bb);
                    break;
                case Effect.ClearNoise: s.Noise = null; break;
                case Effect.Eat:
                    s.Food -= 1;
                    s.Hunger = 0;
                    break;
                case Effect.TakeFood: s.Food += 2; break;
                case Effect.Sleep: s.Fatigue = 0; break;
                case Effect.Score: bb.Score += (ulong)arg; break;
            }
        }

        // The same tree as soldier::spec in ../src/soldier.rs.
        public static Node<Bb> Build() => bt.Selector("soldier",
            bt.Sequence("respawn", C(s => s.Health <= 0), Work(0, 20, Abort.Never, Effect.Respawn)),
            bt.Sequence("survive", C(s => s.Health < 35), bt.Selector("heal",
                bt.Sequence("medkit", C(s => s.Medkits > 0), Work(1, 3, Abort.Never, Effect.UseMedkit)),
                bt.Sequence("cover", C(s => s.Foe), Go(5, false), Work(2, 6, Abort.Never, Effect.Heal, 25)),
                bt.Sequence("base", Go(-20, false), Work(2, 6, Abort.Never, Effect.Heal, 25)))),
            bt.Sequence("combat", C(s => s.Foe), bt.Selector("engage",
                bt.Sequence("empty", C(s => s.Mag == 0), bt.Selector("rearm",
                    bt.Sequence("reload", C(s => s.Reserve > 0), Work(3, 4, Abort.Never, Effect.Reload)),
                    bt.Sequence("melee", C(s => s.FoeDist <= 2), Work(4, 3, Abort.OnFoeGone, Effect.Melee)),
                    bt.Sequence("ammo", Go(-15, false), Do(Effect.TakeAmmo)))),
                bt.Sequence("in range", C(s => s.FoeDist <= 12), bt.Selector("attack",
                    bt.Sequence("grenade", C(s => s.Grenades > 0), C(s => s.FoeGrouped), Do(Effect.Throw)),
                    bt.Sequence("shoot", Work(5, 2, Abort.OnFoeGone, Effect.Idle), Do(Effect.Fire)))),
                Close(12))),
            bt.Sequence("investigate", C(s => s.Noise.HasValue), Go(bb => bb.Soldier.Noise ?? bb.Pos, true),
                Work(6, 3, Abort.OnFoe, Effect.Idle), Do(Effect.ClearNoise)),
            bt.Sequence("eat", C(s => s.Hunger > 400), bt.Selector("food",
                bt.Sequence("has food", C(s => s.Food > 0), Work(7, 5, Abort.OnFoe, Effect.Eat)),
                bt.Sequence("kitchen", Go(20, true), Do(Effect.TakeFood)))),
            bt.Sequence("sleep", C(s => s.Fatigue > 700), Go(-25, true), Work(8, 12, Abort.OnFoe, Effect.Sleep)),
            bt.Sequence("patrol", Go(12, true), Work(6, 3, Abort.OnFoe, Effect.Idle), Go(-12, true),
                Work(6, 3, Abort.OnFoe, Effect.Idle), Go(0, true), Do(Effect.Score, 1)));
    }
}

static class Program
{
    const int SingleTicks = 2_000_000, Agents = 10_000, Frames = 200, Samples = 7;
    static readonly string[] Scenarios = { "select8", "patrol", "guard", "soldier" };

    static int Index(Status s) => s switch { Status.Ok => 0, Status.Fail => 1, _ => 2 };

    static string Checksum(string scenario, Bb bb, long[] counts)
    {
        var s = bb.Soldier;
        return string.Join(" ",
            scenario, counts[0], counts[1], counts[2], bb.T, bb.Mode, bb.Enemy ? 1 : 0, bb.Hp, bb.Pos,
            bb.Charge, bb.Score, s.Health, s.Medkits, s.Mag, s.Reserve, s.Grenades, s.Food, s.Hunger,
            s.Fatigue, s.FoeLeft, s.FoeHp, s.FoeDist, s.FoeGrouped ? 1 : 0,
            s.Noise.HasValue ? s.Noise.Value.ToString() : "-", string.Join(",", s.Work), s.Kills, s.Deaths,
            string.Join(",", s.Effects));
    }

    static double Median(List<double> v)
    {
        v.Sort();
        return v[v.Count / 2];
    }

    static void Run(string lib, string scenario, INodeContextCreator<Bb> creator, int single, int frames, int samples)
    {
        var root = Trees.Build(scenario);

        // Trace: also the first JIT warm-up.
        var bb = new Bb(scenario, 0);
        var ctx = new Context<Bb>(root, bb, creator);
        var counts = new long[3];
        for (int i = 0; i < 100_000; i++)
        {
            bb.StepWorld();
            counts[Index(ctx.Update())] += 1;
        }
        string checksum = Checksum(scenario, bb, counts);

        // One agent. Warm up long enough for tiered compilation to settle.
        for (int i = 0; i < single; i++)
        {
            bb.StepWorld();
            ctx.Update();
        }
        var times = new List<double>();
        for (int s = 0; s < samples; s++)
        {
            var sw = Stopwatch.StartNew();
            for (int i = 0; i < single; i++)
            {
                bb.StepWorld();
                ctx.Update();
            }
            times.Add(sw.Elapsed.TotalNanoseconds / single);
        }
        double nsPerTick = Median(times);
        long before = GC.GetAllocatedBytesForCurrentThread();
        for (int i = 0; i < 100_000; i++)
        {
            bb.StepWorld();
            ctx.Update();
        }
        double bytesPerTick = (GC.GetAllocatedBytesForCurrentThread() - before) / 100_000.0;

        // A population. Blackboards are built first so only the agents' BT
        // state is counted, and measured after ticking, when running paths
        // hold their node contexts.
        var bbs = new Bb[Agents];
        for (int i = 0; i < Agents; i++) bbs[i] = new Bb(scenario, (uint)i * 7919);
        var agents = new Context<Bb>[Agents];
        long mem0 = GC.GetTotalMemory(true);
        var build = Stopwatch.StartNew();
        for (int i = 0; i < Agents; i++) agents[i] = new Context<Bb>(root, bbs[i], creator);
        double buildNs = build.Elapsed.TotalNanoseconds / Agents;
        void Frame()
        {
            for (int i = 0; i < Agents; i++)
            {
                bbs[i].StepWorld();
                agents[i].Update();
            }
        }
        for (int f = 0; f < frames / 5; f++) Frame();
        double bytesPerAgent = (GC.GetTotalMemory(true) - mem0) / (double)Agents;
        times.Clear();
        for (int s = 0; s < samples; s++)
        {
            var sw = Stopwatch.StartNew();
            for (int f = 0; f < frames; f++) Frame();
            times.Add(sw.Elapsed.TotalNanoseconds / ((double)frames * Agents));
        }
        double nsPerAgentTick = Median(times);
        GC.KeepAlive(agents);

        Console.WriteLine(string.Join("\t", lib, scenario, nsPerTick.ToString("F1"), nsPerAgentTick.ToString("F1"),
            bytesPerTick.ToString("F1"), bytesPerAgent.ToString("F0"), buildNs.ToString("F0"), checksum));
    }

    static void Main(string[] args)
    {
        bool quick = args.Contains("--quick");
        int single = quick ? 200_000 : SingleTicks, frames = quick ? 20 : Frames, samples = quick ? 3 : Samples;
        foreach (var scenario in Scenarios)
        {
            Console.Error.WriteLine($"running bt-tree / {scenario}");
            Run("bt-tree", scenario, SimpleNodeContextCreator<Bb>.Instance, single, frames, samples);
            Run("bt-tree-pooled", scenario, PoolNodeContext<Bb>.Instance, single, frames, samples);
        }
    }
}
