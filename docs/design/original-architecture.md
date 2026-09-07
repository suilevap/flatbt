# Rust Behavior Tree — Master Architecture Spec

## 1. Главная идея

BT должен ощущаться не как runtime-граф объектов, а как **resumable executable program**.

При этом архитектура делится на два уровня:

### Low-level/runtime API

Типизированные Rust-комбинаторы:

```rust
select((
    seq((check1(), a(), b())),
    seq((check2(), c(), d(), e())),
    seq((fallback(), wait(5.0))),
))
```

Они:

- полностью статически типизированы;
- используют generic composition;
- не требуют `dyn` для statically-known children;
- являются reference/low-level API;
- позволяют реализовать и проверить всю runtime semantics до появления DSL compiler.

### High-level compiled API

```rust
bt! {
    select {
        {
            check(ctx.can_attack());
            attack();
        }

        {
            move_to_target();
            wait(0.5);
        }
    }
}
```

`bt!` является частью v1, но реализуется **после стабилизации runtime semantics**.

Его цель — не просто красивый синтаксис.

Он может компилировать BT в более эффективный resumable state machine:

- flatten standard control flow;
- inline synchronous conditions;
- не создавать `BtNode` для trivial operations;
- хранить transient locals на обычном stack;
- хранить persistent locals только там, где они действительно переживают suspension.

---

# 2. `BtNode`

`BtNode` — самый низкоуровневый executable semantic protocol.

```rust
enum EntryMode {
    Resume,
    Evaluate,
}

trait BtNode<C> {
    type State: Send + 'static;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        exec: &mut ExecutionCursor,
        mode: EntryMode,
    ) -> NodeResult;
}
```

Точная Rust-signature/lifetimes остаётся implementation-open.

## `EntryMode::Resume`

Продолжить уже выбранную invocation по сохранённой continuation.

## `EntryMode::Evaluate`

Повторно выполнить decision semantics текущей invocation.

`Evaluate` **не означает reset state**.

Примеры:

```text
Sequence.evaluate
    → продолжает active child

Reactive Selector.evaluate
    → начинает priority scan с child 0

DynamicOrder.evaluate
    → заново читает источник текущего order
```

Fresh invocation всегда входит как `Evaluate`.

Node может игнорировать `EntryMode`, если её семантика от revalidation не зависит.

---

# 3. Revalidation

Revalidation — часть execution semantics, но не storage.

v1 внешне поддерживает:

```text
ResumeCurrentPath
ReevaluateFromRoot
```

Позже возможны intermediate checkpoints.

Пример persisted path:

```text
Root
  Combat
    DynamicOrder
      Sequence
        Move [Running]
```

Normal continuation:

```text
Root          Resume
Combat        Resume
DynamicOrder  Resume
Sequence      Resume
Move          Resume/Evaluate согласно suspension semantics
```

Full reevaluation:

```text
Root          Evaluate
...
```

Главный invariant:

> evaluator выбирает revalidation policy; execution layer вычисляет `EntryMode` для конкретных node entries.

`BtNode` не спрашивает у storage, происходит ли сейчас full think.

---

# 4. Execution и storage — разные сущности

## 4.1 Execution layer

Концептуальная сущность:

```rust
ExecutionCursor
```

Она отвечает за:

- traversal по persisted path;
- положение относительно revalidation boundary;
- existing vs fresh child invocation;
- определение `EntryMode`;
- speculative traversal;
- logical continuation changes;
- coordination commit/rollback.

Execution layer может использовать storage.

## 4.2 Storage layer

Концептуальные сущности:

```rust
BtState
FrameStorage
ThinkScratch
```

Storage отвечает только за:

- владение памятью frame-ов;
- typed access;
- allocation;
- alignment;
- destruction;
- persistent/scratch backing;
- physical commit;
- relocation.

Storage **не знает**:

- что такое Sequence/Selector;
- почему node Resume/Evaluate;
- где находится revalidation boundary;
- что такое full think.

Зависимость:

```text
ExecutionCursor
      ↓
FrameStorage
```

но не наоборот.

---

# 5. Static dispatch

Static child dispatch является архитектурным требованием.

Для statically-known children нельзя незаметно переходить к:

```rust
&dyn BtNode
```

Concrete/generated parent сам статически вызывает concrete child.

Например:

```rust
match child_index {
    0 => run_child_static(&self.children.0, ...),
    1 => run_child_static(&self.children.1, ...),
    2 => run_child_static(&self.children.2, ...),
    _ => unreachable!(),
}
```

Generic helper допустим:

```rust
fn run_child_static<N: BtNode<C>>(...)
```

Он monomorphized и не создаёт virtual dispatch.

`DynBtNode` используется только на явных runtime-dynamic boundaries.

---

# 6. Tuple как representation heterogeneous children

Для low-level generic backend children представляются обычными Rust tuple:

```rust
(Check1, A, B)
```

Тип:

```rust
(Check1, A, B)
```

никакой собственный HList не требуется.

Поскольку stable Rust не имеет variadic generics, библиотека один раз генерирует impl-ы для tuple arities, например 1–32.

Концептуально:

```rust
trait BtChildren<C> {
    const LEN: usize;

    fn run_child(
        &self,
        index: usize,
        ...
    ) -> NodeResult;
}
```

Для `(A, B, C)` implementation содержит:

```rust
match index {
    0 => run_static(&self.0, ...),
    1 => run_static(&self.1, ...),
    2 => run_static(&self.2, ...),
    _ => unreachable!(),
}
```

Это shared library machinery, а не generated type per BT.

---

# 7. `ControlNode`

Generic runtime representation:

```rust
struct ControlNode<P, Children> {
    policy: P,
    children: Children,
}
```

Стандартный combinator API:

```rust
seq((a(), b(), c()))

select((
    branch1,
    branch2,
    branch3,
))
```

Итоговый тип может быть большим:

```text
ControlNode<
    Selector,
    (
        ControlNode<Sequence, (...)>,
        ControlNode<Sequence, (...)>,
        ...
    )
>
```

но пользователю его никогда не требуется писать вручную.

---

# 8. `BtControl`

`BtControl` — **не `BtNode`**.

Это compile-time control-flow policy.

Концептуально:

```rust
trait BtControl<C> {
    type State: Default + Send + 'static;
    type ChildMeta;

    fn begin(...) -> ControlOp;
    fn child_succeeded(...) -> ControlOp;
    fn child_failed(...) -> ControlOp;

    fn child_committed(...) {}
}
```

```rust
enum ControlOp {
    RunChild(usize),
    Success,
    Failure,
}
```

`BtControl` возвращает logical child index.

`ControlNode` статически dispatch-ит соответствующий concrete child.

Custom `BtControl` внутри generic/static tree не должен вводить virtual dispatch.

---

# 9. `EntryMode` и `BtControl`

`EntryMode` принадлежит `BtNode`, а не `BtControl`.

Generic `ControlNode::update` делает:

```text
Resume
    → не вызывает policy.begin()
    → следует framework-owned active_child

Evaluate
    → вызывает policy.begin()
```

Сам вызов `begin()` уже является сигналом:

> control decision point сейчас revalidated.

Policy не нужен отдельный `EntryMode`.

---

# 10. Control state

Framework-owned continuation:

```rust
struct ControlState<S> {
    inner: S,
    active_child: Option<usize>,
}
```

`active_child` меняет framework.

Control policy может использовать `inner` и при необходимости читать continuation metadata.

---

# 11. Sequence

Sequence memoryful.

На fresh entry:

```text
child 0
```

На existing invocation:

```text
active_child
```

При `Evaluate` существующий Sequence **не обязан сбрасываться на child 0**.

Его `begin()` может вернуть:

```rust
RunChild(state.active_child.unwrap_or(0))
```

Семантика:

```text
Success → следующий child
Failure → Sequence Failure
Running → сохранить active child
```

Если running child завершился `Success`, Sequence может продолжить следующий child в том же external update.

---

# 12. Reactive Selector

На Resume:

```text
framework следует active_child
begin() не вызывается
```

На Evaluate:

```text
begin() → child 0
```

Семантика:

```text
Success → Selector Success
Failure → следующий child
Running → сохранить active child
```

Full reevaluation может:

```text
old child1 Running

child0 Evaluate → Failure
child1 Evaluate → reuse old invocation/state
```

Existing state и `EntryMode` — разные вещи.

---

# 13. Invocation identity

Возможны три комбинации:

```text
existing frame + Resume
existing frame + Evaluate
fresh frame + Evaluate
```

`fresh + Resume` не имеет смысла.

При terminal result reused invocation заканчивается.

Если control затем снова выбирает тот же child index:

```text
RunChild(same_index)
```

это уже новая invocation.

---

# 14. DynamicOrder

`DynamicOrder` — пример custom low-level `BtNode`, не `BtControl`.

State:

```rust
struct DynamicOrderState {
    selected: BehaviorHandle,
}
```

Resume:

```text
использовать state.selected
```

Evaluate:

```text
заново прочитать current order из context/BB
```

Псевдо:

```rust
match mode {
    EntryMode::Resume => use_saved(),
    EntryMode::Evaluate => choose_again(),
}
```

Persisted selected behavior должен иметь owned stable identity:

- `Arc`;
- asset handle;
- behavior handle;
- stable ID + compatible lookup.

Core invariant:

> persisted state никогда не передаётся несовместимой dynamic definition.

---

# 15. Любой `BtNode` может suspend

`Running` не является action-specific feature.

Любой custom node может:

```text
Running
Running
Success
```

через несколько external updates.

Отдельного `resume()` метода нет.

Всё проходит через:

```rust
update(..., EntryMode)
```

---

# 16. `BtTick`

`BtTick` — отдельная optional capability:

```rust
trait BtTick<C>: BtNode<C> {
    fn tick(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
    );
}
```

`Running` означает suspension.

`BtTick` означает:

> эта node хочет post-commit execution.

Только `tick()` гарантированно выполняется **после выбора resulting Running continuation**.

---

# 17. Speculative side effects

`BtNode::update` является decision/speculative phase.

Поэтому:

- `BtNode::update`;
- `BtAction::start`;
- `BtAction::is_in_progress`;
- `BtAction::complete`

могут выполняться на speculative branch, который потом проиграет.

Только:

```rust
BtTick::tick
```

имеет post-commit guarantee.

Необратимые gameplay effects, зависящие от окончательного выбора branch, должны происходить в `tick`.

---

# 18. `NodeResult`

Conceptual:

```rust
enum NodeResult {
    Success,
    Failure,
    Running {
        tick: Option<ActiveRef>,
    },
}
```

Точная representation implementation-open.

Running может не иметь Tick target.

---

# 19. `ActiveRef`

`ActiveRef` transient.

Живёт только:

```text
update result
→ logical commit
→ immediate tick
```

Не нужен для resumability.

Он может создаваться только framework-ом для текущего concrete frame:

```text
N
N::State
N: BtTick<C>
```

Safe user code не может создать произвольную пару:

```text
NodeA + StateOfNodeB
```

---

# 20. `BtAction`

High-level lifecycle abstraction:

```rust
trait BtAction<C> {
    type State: Send + 'static;

    fn start(&self, ctx: &mut C) -> Option<Self::State>;

    fn is_in_progress(
        &self,
        state: &Self::State,
        ctx: &C,
    ) -> bool;

    fn tick(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
    );

    fn complete(
        &self,
        state: &Self::State,
        ctx: &mut C,
    ) -> bool {
        true
    }
}
```

Adapter state:

```rust
struct ActionNodeState<S> {
    action: Option<S>,
}
```

Lifecycle:

```text
Start
→ None
    Failure

Start
→ Some(state)
→ IsInProgress

true
    Running + Tick target

false
    Complete immediately
    Success/Failure
```

No PendingComplete phase.

---

# 21. Runtime state

Definition и runtime state разделены.

Definition:

```text
immutable code/config
может быть shared между агентами
```

Runtime:

```text
mutable invocation state конкретного агента
```

Persistent state содержит только текущий execution path, а не state всех потенциальных nodes.

---

# 22. Threading

Frame state:

```rust
State: Send + 'static
```

`Sync` не требуется.

Один `BtState` всегда используется эксклюзивно, хотя между updates может попадать на разные worker threads.

Core `BtState` должен намеренно быть:

```text
Send + !Sync
```

Точная marker implementation-open.

Bevy integration может использовать exclusive-access wrapper.

---

# 23. Prototype storage

Первый runtime backend намеренно простой:

```text
one Box per active frame
```

Преимущества:

- frame address stable;
- parent borrow не инвалидируется descendant allocation;
- alignment решён Rust allocator;
- arbitrary owning state;
- Drop простой;
- легко проверить execution semantics.

Это reference/debug backend, а не production representation.

Он может остаться в библиотеке для differential testing.

---

# 24. Production storage target

После стабилизации semantics boxed storage заменяется на:

```text
inline persistent capacity
+
stable overflow segments
+
reusable scratch
```

Requirements:

- existing frames не двигаются во время traversal;
- BtState movable between external updates;
- no persistent raw pointers;
- proper alignment;
- arbitrary `Send + 'static` frame state;
- logical `StateOffset`;
- relocation only at quiescent points;
- standalone destruction.

---

# 25. Drop infrastructure

Каждый owning storage содержит sparse destructor metadata:

```rust
struct DropEntry {
    state: StateOffset,
    drop_fn: DropThunk,
}
```

Только:

```rust
needs_drop::<S>()
```

frames регистрируются.

Нужно поддержать:

- reverse-order destruction;
- scratch rollback;
- persistent suffix discard;
- commit ownership transfer exactly once;
- ZST with Drop;
- standalone `BtState::drop`.

Memory safety не зависит от semantic cancel traversal.

---

# 26. Logical и physical commit

## Logical commit

Execution layer решил:

```text
этот Running branch является resulting continuation
```

После этого sibling уже не может победить в текущем traversal.

`child_committed` относится к этому моменту.

## Physical commit

После unwind frame borrows storage:

- удаляет old divergent suffix;
- переносит candidate state;
- переносит DropEntry ownership;
- при необходимости compact/relocate.

Эти две фазы не должны смешиваться.

---

# 27. Cancellation

Native cancel/abort hooks отсутствуют в v1.

Preemption гарантирует только normal Rust Drop discarded state.

Gameplay cleanup строится через:

- Tick-confirmed effects;
- RAII;
- leases;
- reconciliation;
- game-level ownership.

---

# 28. Unsafe boundary

Unsafe локализован в framework internals:

- heterogeneous frame storage;
- alignment;
- logical→physical resolution;
- typed reconstruction;
- relocation;
- drop thunk;
- erased dynamic adapters где необходимо.

User-facing APIs безопасны:

```text
BtNode
BtTick
BtAction
BtControl
combinators
bt!
```

---

# 29. `bt!` — compiled frontend

`bt!` остаётся обязательной частью v1.

Но он реализуется **в конце**, после runtime/combinator backend.

Он не обязан сохранять node-for-node graph representation.

Главная цель:

> компилировать BT DSL в resumable executable code.

---

# 30. Standard control-flow flattening

Для standard `Sequence` и `Selector` macro может строить flat CFG.

Compiler primitive:

```text
compile(node, on_success, on_failure)
```

## Sequence

Для:

```text
A
B
C
```

edges:

```text
A.Success → B
A.Failure → Sequence.Failure

B.Success → C
B.Failure → Sequence.Failure

C.Success → Sequence.Success
C.Failure → Sequence.Failure
```

## Selector

Для:

```text
select {
    A
    B
    C
}
```

edges:

```text
A.Success → Selector.Success
A.Failure → B

B.Success → Selector.Success
B.Failure → C

C.Success → Selector.Success
C.Failure → Selector.Failure
```

Running:

```text
save resume label
return Running
```

---

# 31. Generated flat state machine

Conceptually:

```rust
enum Pc {
    Start,
    A,
    B,
    C,
    D,
    Wait,
}
```

Generated node:

```rust
loop {
    match pc {
        Pc::Start => { ... }
        Pc::A => { ... }
        Pc::B => { ... }
        ...
    }
}
```

Normal resume может стать:

```text
load resume_pc
→ jump directly to suspended point
```

вместо:

```text
Root Resume
→ Selector Resume
→ Sequence Resume
→ leaf Resume
```

Standard Sequence/Selector frames могут исчезнуть полностью, потому что их continuation уже закодирована в PC.

---

# 32. Почему explicit flattening может быть нужен

Nested generic combinators позволяют compiler-у inline-ить function calls.

Но compiler вряд ли автоматически преобразует persistent state:

```text
Selector.active_child
Sequence.active_child
```

в единый:

```text
Pc::D
```

между external updates.

Поэтому explicit CFG lowering остаётся потенциально полезной optimization.

Тем не менее до реализации flattening generic backend должен быть benchmarked.

---

# 33. Inline synchronous code в `bt!`

Не всё внутри `bt!` обязано быть `BtNode`.

Например:

```rust
bt! {
    check(ctx.has_target());
    shoot();
}
```

может стать:

```rust
if !ctx.has_target() {
    goto_failure;
}

run_shoot();
```

`check` не требует:

```text
Check<Closure>
BtNode frame
BtNode state
```

Это важная часть идеи compiled BT.

---

# 34. Locals

`bt!` может содержать обычные локальные вычисления.

Например:

```rust
bt! {
    let distance = ctx.distance();
    check(distance < 10.0);
    shoot();
}
```

Если `distance` не нужен после suspension point, он остаётся обычным Rust stack local.

Никакого persistent state для него не создаётся.

---

# 35. Locals через suspension

Пример:

```rust
bt! {
    let target: Entity = ctx.best_target();

    move_to(target); // может Running

    shoot(target);
}
```

`target` live across suspension, поэтому должен стать частью generated persistent state.

Conceptually:

```rust
struct GeneratedState {
    pc: Pc,
    target: Entity,
}
```

В v1 такой persistent local может требовать explicit type annotation:

```rust
let target: Entity = ...
```

потому что proc macro не имеет полноценного rustc type inference для generated fields.

---

# 36. Граница сложности `bt!`

v1 должен поддерживать контролируемый DSL, а не пытаться стать новым `async fn`.

## Реалистично для v1

- Sequence;
- Selector;
- BtNode calls;
- static dispatch;
- flattened CFG;
- inline conditions;
- explicit `success` / `failure`;
- простые `if`;
- synchronous expressions;
- ordinary locals, не переживающие suspension;
- typed persistent locals через suspension.

## Не требуется в v1

- arbitrary loops с suspension внутри;
- arbitrary `match` с complex persistent bindings;
- borrowed locals across suspension;
- iterator state lowering;
- полноценная Rust coroutine semantics;
- automatic inference типов generated persistent fields;
- arbitrary Rust control flow как внутри `async fn`.

Главный принцип:

> `bt!` выглядит как код, но имеет контролируемую grammar и BT-specific suspension semantics.

---

# 37. Custom controls и flattening

Standard:

```text
Sequence
Selector
```

могут быть compiler primitives и flatten-иться.

Custom `BtControl` в первой версии compiled frontend может быть optimization boundary.

То есть:

```text
flattened standard region
    ↓
CustomControlNode
    ↓
flattened standard child region
```

Custom control всё ещё может использовать statically-known heterogeneous children без dyn dispatch.

Позже возможно opt-in compile-time lowering protocol для custom controls, но это не requirement v1.

---

# 38. Dynamic boundaries в `bt!`

Explicit runtime-dynamic behavior остаётся boundary:

```text
flattened static code
    ↓
DynamicOrder / DynBtNode
    ↓
runtime-selected behavior
```

`bt!` не обязан пытаться flatten-ить opaque runtime behavior.

---

# 39. Milestones

Каждый milestone должен завершаться реально исполняемым BT-примером, а не только внутренними unit tests.

## M0 — synchronous generic runtime

Без `bt!`.

Implement:

- `BtNode`;
- Success/Failure;
- `BtControl`;
- `ControlNode`;
- tuple children;
- Sequence;
- Selector;
- custom BtControl;
- static dispatch.

No Running/state stack.

Deliverable:

```rust
select((
    seq((check1(), a(), b())),
    seq((check2(), c(), d())),
))
```

реально выполняется.

---

## M1 — Running + normal resume

Add:

- persistent state;
- `EntryMode`;
- `ResumeCurrentPath`;
- boxed frame storage.

Deliverable:

```text
check
wait_frames(3)
fire
```

работает across updates.

---

## M2 — root revalidation + speculation

Add:

```text
ReevaluateFromRoot
```

Required:

- reactive selector reuse;
- speculative losing branch;
- preemption;
- existing state + Evaluate.

Deliverable:

runtime priority change visibly preempts old behavior.

---

## M3 — `BtTick`

Add post-commit execution.

Required:

- losing candidate never ticks;
- committed Running node ticks exactly once per external update.

---

## M4 — `BtAction`

Add action lifecycle adapter.

Deliverable:

small multi-frame AI using Move/Aim/Fire/Wait.

---

## M5 — dynamic composition

Add:

- `DynBtNode`;
- BehaviorHandle;
- DynamicOrder.

Deliverable:

runtime high-level order switching while static subtrees remain statically dispatched.

---

## M6 — production storage

Replace boxed backend with:

- inline storage;
- stable overflow;
- scratch;
- logical offsets;
- DropEntry;
- rollback;
- suffix discard;
- physical commit;
- ZST/alignment support;
- panic guards.

All behavioral tests M1–M5 remain unchanged.

---

## M7 — ECS/Bevy integration

Add:

- movable BtState;
- Send + !Sync;
- ECS wrapper;
- archetype relocation tests;
- despawn/destruction tests.

---

## M8 — `bt!` compiler frontend

Still v1.

### M8.1 — basic DSL

Support:

- Sequence;
- Selector;
- BtNode invocation;
- success/failure/check constructs.

First implementation may initially lower close to generic semantics.

### M8.2 — standard CFG flattening

Compile Sequence/Selector to flat state machine.

Benchmark against generic backend.

### M8.3 — synchronous inline code

Inline checks and synchronous expressions directly into generated control flow.

Avoid wrapper BtNodes where unnecessary.

### M8.4 — locals

Support ordinary stack locals.

### M8.5 — typed locals across suspension

Promote only live-across-suspend locals into generated persistent state.

Do not attempt full Rust coroutine semantics.

---

# 40. Critical tests

1. Plain BtNode may suspend without Tick.
2. Fresh invocation enters as Evaluate.
3. Sequence preserves active child across revalidation.
4. Reactive Selector Resume follows active child.
5. Reactive Selector Evaluate starts priority scan again.
6. Existing invocation may receive Evaluate.
7. DynamicOrder Resume uses saved selection.
8. DynamicOrder Evaluate rereads source.
9. Suspended child can complete and next child runs in same update.
10. Higher-priority candidate can preempt old branch.
11. Losing speculative branch never ticks.
12. Tick occurs after logical commit.
13. Repeated same child after terminal result is fresh invocation.
14. child_committed only occurs on actual policy commit.
15. Static tuple children have no virtual dispatch.
16. Custom BtControl remains statically dispatched.
17. Standalone BtState Drop works.
18. Scratch rollback drops exactly once.
19. Commit transfers ownership exactly once.
20. ZST custom Drop works.
21. Parent references survive descendant storage growth.
22. Dynamic behavior state never reaches incompatible definition.
23. BtState may move between external updates.
24. BtState is Send but not Sync.
25. Generic and compiled `bt!` implementations have equivalent observable behavior.
26. Flattened `bt!` Resume does not require walking all standard static ancestors.
27. Inline `check` in `bt!` creates no runtime node/frame.
28. Non-persistent local stays stack-local.
29. Typed local live across suspension survives correctly.

---

# 41. Current implementation-open items

- exact `BtNode::update` signature/lifetimes;
- exact `EntryMode` representation;
- whether Resume/Evaluate eventually deserve distinct restricted execution façades;
- `ExecutionCursor` API;
- child-entry API;
- behavior/state schema binding;
- boxed frame representation;
- final StateOffset encoding;
- inline capacity;
- overflow segment layout;
- scratch scope API;
- logical/physical commit algorithm;
- ActiveRef representation;
- DynBtNode API;
- panic guards;
- !Sync marker;
- Bevy wrapper;
- tuple arity limit;
- DSL grammar;
- generated CFG representation;
- persistent-local syntax/type rules;
- compiled-vs-generic benchmark threshold.

---

# 42. Current locked direction

The working low-level API is:

```rust
BtNode::update(..., EntryMode)
```

not separate `resume()` / `evaluate()` methods.

Physical frame storage and revalidation/execution traversal are separate abstractions.

Generic composition uses:

```text
ControlNode<P, TupleChildren>
```

and is a permanent low-level/reference API.

`bt!` is also part of v1, but is implemented last.

It is treated as:

> a BT-specific resumable compiler frontend,

not merely:

> syntax sugar for nested generic combinators.

The project deliberately validates BT semantics first with simple stable boxed storage, then optimizes memory representation, and only afterwards introduces compiled `bt!` lowering.