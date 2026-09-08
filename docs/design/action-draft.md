# Inline actions with cancellation owned by state

This document describes the current inline action lifecycle and optional
cancellation helpers. The post-commit target experiment remains on
`experiment/post-commit-actions` (`95f9e33`). Its transient target protocol is
independent of the cancellation approach described here.

## Action lifecycle

BtNode retains update, State, and NodeResult. ActionNode stores Option<A::State>:
start initializes it, is_in_progress observes it, optional tick performs inline
work, and complete returns success/failure once progress finishes. Action state
needs Send + 'static but not Default. Tick has an empty default for externally
scheduled actions. Complete receives &mut State so an action can finish/disarm
its owned handles before destruction.

An existing invocation does not restart just because entry mode is Evaluate.
When is_in_progress returns false, complete runs immediately; a parent Sequence
can start and tick the next action in the same update. There is no PendingComplete
phase. All callbacks run during traversal and may be speculative. Tick effects
are not rolled back if a parent later rejects the candidate.

## Cancellation through Drop

Cancellation belongs to the action's state or its owned resources. A state may
own a cancel-on-drop request handle. Dropping that state then requests cancellation
without any call through BtNode, BtChildren, controls, or root execution. A state
that needs no cancellation carries no cancellation flag, handle, or callback.
The normal Rust destruction of non-resource states can be eliminated by the
compiler; there is no additional library-wide cancel traversal or check.

Normal completion also drops state. If cancellation should only happen when an
invocation is abandoned, complete must disarm its handle, or the handle must make
cancellation of already-finished work harmless. The example disarms explicitly,
including after observing external failure. Default complete cannot know how to
disarm an arbitrary application resource.

The existing enum replacement already supplies preemption: a new Running
candidate replaces the old variant and drops its payload. Rejected candidates,
terminal revalidation, reset(), and dropping BtState also destroy the appropriate
state automatically. Custom composing nodes need no cancel forwarding, but still
own correct state lifetimes if they store descendants themselves. A new start or
inline tick can execute before the old state's destructor; cancellation must
refer specifically to the old operation.

Drop gets no blackboard parameter. State must own everything necessary to request
cancellation. Avoid putting the whole BB behind a shared lock solely for this:
Drop may run inside update while the caller already holds that lock. Request
handles, cancellation tokens, or an independently owned command sender avoid
this reentrant-lock problem. The current State: Send bound excludes Rc and
Rc<RefCell<_>>. An Arc is possible when its contents satisfy the necessary
thread-safety bounds, but sharing/locking cost belongs only to users choosing it.

Do not store the same owning cancellation state back inside its own scheduler or
BB: keep observer/worker handles separate to avoid ownership cycles. The example
has one cancellation owner and an independent worker-side observer.

## Optional cancellation helper

CancelOnDrop<T> stores a value and Option<fn(&mut T)> inline. Start can define the
paired cancellation directly with CancelOnDrop::new(request, |request| { ... }).
The callback takes the concrete value and cannot capture other variables; any
required cancellation data is stored in that value. No BtCancel implementation
is required for this form.

BtCancel remains available for request handles with reusable cancellation logic.
CancelOnDrop::from(handle) uses that handle's BtCancel::cancel implementation.
Both forms share the same wrapper and cleanup mechanism. Users who already own
a cancellation guard can continue to use it directly without this helper.

The destructor invokes the callback if present. Disarm clears it, preserving
ordinary destruction of T. Deref/DerefMut expose the concrete payload. The optional
function pointer also represents the armed state, so there is no separate bool.
No allocation or hook on BtNode/BtAction is added. The function pointer may be
called indirectly on cancellation; it is not invoked on Resume or after disarm.

The external example uses CancelOnDrop<RequestHandle> as BtAction::State. Start
submits the scheduler's request and defines its cancellation immediately beside
it. Complete handles the outcome and calls disarm. Disarm is explicit because
the library cannot decide whether an application still needs cancellation after
observing its terminal outcome.

## External work without per-frame BT updates

Start asks the external scheduler to launch an operation and returns its handle.
Is_in_progress observes pending/running status; complete reads the outcome and
finishes the handle. The external system keeps working between BT updates.
Applications can use periodic Evaluate for reactivity and Resume on completion.
No async runtime, futures, wakeup queue, or mandatory boxing is imposed by core.

The external_action example models movement as a component. The external system
advances it every frame; BT runs at initial start, periodic preemption on frame 4,
and completion on frame 7. It is engine-independent and does not spawn threads.
The action does not implement tick. Its CancelOnDrop<RequestHandle> invokes the
callback on destruction, setting a request-specific atomic flag; the external
system observes it and removes the component later.
Thus reset requests cancellation immediately, without needing ctx or waiting for
another BT update; application scheduling determines when cancellation takes effect.

The example scheduler allocates one Arc<AtomicBool> for each new external request.
There is no allocation or Arc clone per Resume update. This is an application
choice, not BT storage: a scheduler can supply pooled/generational handles instead.
Distinct tokens ensure an old destructor cannot cancel a newly installed operation.

Completion events should match the current request and an active BT instance.
An old event must not restart an idle tree or finish a replacement. Start is
immediately followed by is_in_progress, so accepted but queued work must count as
active even before a deferred component is installed. The example installs its
component immediately and retains terminal status until the BT observes it.
Real deferred-command integrations must supply their own pending/outcome tracking.
Disappearance of an operation is not automatically success; the example returns
Failure if its request was removed or replaced externally.

## Validation and cost

Action traces distinguish complete from cancel-on-drop. They cover failed start,
immediate completion with either result, continuing a Sequence, rejected Running
candidates, preemption, terminal revalidation, repeated reset, and dropping the
whole tree. External-action tests cover work progressing without BT updates,
retaining progress on Evaluate, cancellation on reset, a replacement surviving
old-state destruction, and external removal producing Failure.

Cancellation adds no callbacks or dispatch to controls, children, or execution.
It relies on the destruction needed by each actual state type. CancelOnDrop
stores an optional function pointer and checks it during destruction; other
states pay only for their own resources. Resource-owning states still have their
real destructor cost and may affect layout/code generation. This is not a promise
of identical timings for arbitrary heterogeneous trees.
