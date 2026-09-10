# Actions and cancellation

Implemented draft. All callbacks run inline. The
[post-commit experiment](../../experiments/README.md) is archived separately.

## Lifecycle

`action(value)` adapts `BtAction` to `BtNode`, storing `Option<A::State>`.
Action state requires `Send + 'static`; no `Default` bound.

| Callback | Contract |
| --- | --- |
| `start` | Called once on entry. None fails; Some creates action state. |
| `is_in_progress` | Called immediately after start and on later updates. True runs tick; false runs complete. |
| `tick` | Inline work, then Running. Defaults to no work for external operations. |
| `complete` | Returns success/failure before state drops. Receives `&mut State` to finish or disarm resources. |

Evaluate preserves an existing action. Completion is immediate; Sequence may
start and tick the next action in the same update. No PendingComplete phase.
Every callback may run on a candidate later rejected. Effects persist.

## Cancellation through Drop

State owns cancellation resources. Destruction on rejection, preemption, terminal
result, reset, or BtState Drop requests cancellation. No cancel traversal or
callback is added to nodes, controls, or execution. Custom composers still own
the lifetimes of their descendant state.

Normal completion also drops state. Disarm after handling either terminal outcome,
or make cancellation harmless for finished work. Default `complete` cannot disarm
an arbitrary resource. New candidate start/tick may run before old-state Drop;
cancellation must identify the old request specifically.

Drop receives no context. Store a token, request handle, or command sender in state.
Avoid locking the entire blackboard from Drop: update may already hold that lock.
`State: Send` excludes `Rc` and `Rc<RefCell<_>>`; Arc contents must satisfy
the required thread-safety bounds. Keep cancellation owners separate from scheduler
observers to avoid ownership cycles.

## `CancelOnDrop<T>`

| API | Use |
| --- | --- |
| `CancelOnDrop::new(value, cancel_fn)` | Supply a function or non-capturing closure. Keep callback data in `value`. |
| `CancelOnDrop::from(value)` | Use the value's `BtCancel::cancel` implementation. |
| `disarm()` | Suppress cancellation; ordinary T destruction still runs. |
| `Deref` / `DerefMut` | Access the wrapped value. |

Stores T and `Option<fn(&mut T)>` inline. The optional pointer also tracks armed
state: no allocation or separate flag. Drop may call the function indirectly;
Resume and disarmed Drop do not call it. Existing application guards need no wrapper.

## External work

Start submits work and returns a handle. Progress observes pending/running work;
complete reads the outcome. The scheduler runs between BT updates. Use periodic
Evaluate for reactivity and Resume on completion. Core requires no async runtime,
futures, wakeup queue, threads, or boxing.

The [external action example](../../examples/external_action.rs) advances movement
for ten external frames with BT updates at start, preemption on frame 4, and
completion on frame 7. It installs the component immediately and keeps terminal
status until observed. It implements no tick.

Its request-specific `Arc<AtomicBool>` allocates once per submission, with no
allocation or Arc clone on Resume. Drop sets the flag; the external system removes
the component later. Reset therefore requests cancellation immediately, while the
scheduler determines when it takes effect. Pooled/generational handles can replace
this application-owned allocation.

Integration requirements:

- Match completion events to the current request and an active BT instance.
  Stale events must not restart idle trees or finish replacements.
- Treat accepted queued work as active: progress runs immediately after start.
- Track pending status and terminal outcomes when using deferred commands.
- Do not equate disappearance with success. The example fails if work was removed
  or replaced externally.

## Validation and cost

[Action tests](../../tests/action.rs) cover failed start, immediate success/failure,
sequence continuation, candidate rejection, preemption, terminal revalidation,
reset, and Drop. [External tests](../../tests/external_action.rs) cover independent
progress, Evaluate reuse, reset cancellation, replacement safety, and removed work.

Cancellation costs belong to the actual state type. `CancelOnDrop` adds its pointer
and a Drop check; resource destructors may affect layout and generated code.
No uniform timing guarantee across state types.
