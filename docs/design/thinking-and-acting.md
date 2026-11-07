# Thinking and acting

This is the execution contract of the public API. It applies to every node,
control, action, order, scope, and builder. The canonical API contract is
[`BtNode`](../../src/runtime/node.rs); the runnable example is
[`thinking`](../../examples/thinking.rs).

## Thinking retains information

`Blackboard` is the agent's perception and mutable working memory. It is supplied by
the application on each update. Visiting a branch performs real work on that
blackboard: recording an attempted route, caching an answer, updating a preference,
or advancing a random generator. A later choice may use those writes.

The blackboard represents what the AI knows about the world. Examples put gathered
facts in `observed`, with reasoning stored separately. Nodes read observed
ammunition and position while considering options; they do not decrement that
ammunition to simulate a proposed shot or write a destination as if they had
arrived. Predictions and planned state belong in separate reasoning fields.
The driver applies the final act to gameplay state. Gather then refreshes the
AI's observations from the actual outcome, retaining useful reasoning.

`Failure` means that an invocation did not work out. Replacing a branch means
that another invocation was selected. Both retain writes to the blackboard and node
`Memory`; neither rolls back thinking. This is intentional API behavior.

| Storage | Owner | Lifetime |
| --- | --- | --- |
| `Blackboard` | Application | As long as the application retains it; the tree never resets it |
| Node `Memory` | Driver's tree state | Across invocations and branch replacement, until `forget` or drop |
| Node `State` | Invocation | While that invocation runs; dropped on completion or replacement |
| Scope locals | Scope invocation | Survive suspension; dropped when their scope ends |

The core and Bevy APIs use `Blackboard` for this role. It is an application-defined,
statically typed value shared by the nodes of one agent. It carries perception
and working memory, with no read-only or transactional behavior implied by the name.

## Acting follows the final result

`Running(act)` proposes what the agent should be doing. Inner nodes can produce
acts that a parent discards. The driver applies only the act in the root's
final result to gameplay state, after the update returns. A terminal root has
no act to apply. Bevy publishes that final decision as the act component.

Node callbacks keep gameplay changes in the driver. They can start owned
planning computations, such as path queries, before a branch is selected.
Dropping invocation state releases or requests cancellation of those resources
according to their own contract. Cancellation does not erase thoughts or undo
completed work. The Rust types do not enforce application-level purity.

`Resume` and `Evaluate` both use this contract. Bevy's `Behavior::tick` can run
a failed resumed invocation followed by a fresh evaluation in the same tick;
the second pass sees the blackboard and memory writes from the first. Only the
final act is published. This is one tick with up to two tree passes.

## Maintaining the API

Keep the mutable blackboard and final act visible in trait signatures, builder docs,
and examples. A failed alternative that leaves a thought is an expected
outcome, not a transaction failure. Changes to selection or retry policies
must preserve this distinction or explicitly change the contract.
