# Inline action lifecycle draft

This draft extends the simple static-state baseline at `2f1525c`. The earlier
post-commit target design is committed on `experiment/post-commit-actions`
(`95f9e33`); see [experiments](../../experiments/README.md).

## Ordinary node execution

BtNode keeps its existing update method, associated State, and NodeResult.
Controls, tuple dispatch, and root execution do not acquire target types or
another traversal phase. There is no separate BtTick trait in this draft.

BtAction keeps start, is_in_progress, tick, and complete. `action(value)` wraps
it in ActionNode, whose ordinary node state is Option<A::State>. The option
starts empty, so A::State needs Send + 'static but does not need Default.

Each ActionNode::update performs:

1. If empty, call start. None means immediate Failure.
2. Query is_in_progress on the stored action state.
3. If true, call tick immediately and return Running.
4. If false, call complete immediately, drop state, and return Success or Failure.

An existing invocation never starts again merely because entry mode is Evaluate.
Tick progress is checked by the next update; there is no second progress query
in the same call. When that query reports completion, Sequence can continue to
another action and tick it within the same external update. There is no
PendingComplete phase. Reset and preemption perform ordinary Rust destruction;
complete is not a cancellation callback.

## Deliberate semantic tradeoff

All callbacks now execute within ordinary traversal, including tick. They receive
mutable context except the read-only is_in_progress query. Effects are immediate
and are not rolled back when a branch loses. This explicitly relaxes the earlier
selected-only/post-commit guarantee at the user's request for a simpler main API.

For example, an enclosing node can call a child action, receive Running after
its tick, then reject it with Failure. The tick has already happened. Multiple
actions can therefore tick during one external update if composition explores
and rejects them. A new candidate may also tick before old state is dropped.

This is not equivalent to the archived experiment. Tests record the changed
ordering and the visible effect of a rejected Running candidate. Applications
must account for speculative effects when choosing their composition.

## Scope and validation

The core addition is one lifecycle trait and its adapter. No heap allocations,
unsafe code, frame stack, active-target metadata, or dispatch traits are added.
Future dynamic composition still needs state erasure and identity rules, but
there is no additional transient target protocol to erase at that boundary.

Five action tests cover rejected start, immediate completion with either result,
completion followed by the next action's tick, reevaluation and preemption, and
an enclosing node rejecting a candidate after its inline tick. Existing static
composition tests are unchanged. The action example shows Move finishing and
Fire ticking in the same update. A standalone allocator probe measured zero
allocations across construction and 2000 updates with completion and preemption.
