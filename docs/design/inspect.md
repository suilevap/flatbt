# Inspection

Debug views of a tree and its invocation state: the running path, node names,
scope locals, policy progress.

## Protocol

`BtNode::inspect(&self, state: Option<&Self::State>, &mut dyn Inspector)` is a
default method, so every existing node is inspectable with no work: the default
reports a childless node named by its type. `state` is `Some` exactly for nodes
on the running path, since only they hold invocation state; a composing node
passes each child the state it saves for it, and `None` to the rest. The same
walk therefore gives the running path and the whole definition.

An `Inspector` receives `enter(NodeInfo)`, fields, children, `exit`. `enter`
returning false skips a subtree, so a path-only view never walks inactive
branches. `dyn Inspector::node` keeps the order for implementors.

`BtChildren::inspect_children` is required: its implementors are the crate's
tuples and `Ordered`. `BtControl` adds `kind` and `inspect` with defaults, so
custom policies need nothing.

Nothing here touches `update`: the methods are only instantiated when called,
so trees that are never inspected compile to the same code. No allocation, no
`std`: text goes through `core::fmt`.

## Names

`NodeInfo` carries three strings:

- `kind`: the constructor (`seq`, `leaf`, `guard`), or the type name for a
  custom node or policy.
- `name`: from `.named(..)`, or from code. `core::any::type_name` of a function
  item is its path, so `check(has_ammo)` is named `has_ammo`; closures are
  `{{closure}}` and get none. An action is named by its type.
- `label`: what the parent calls the child. `choose!` and `per_child!` wrap each
  arm with `inspect::label(stringify!(pattern), node)`.

Name and label are separate so an arm's pattern does not replace a name the
author gave inside it. `Named` wraps the node and renames the next `enter`
through an adapter inspector; nesting lets the outer name win, so an explicit
`.named` replaces a name taken from code.

`#[track_caller]` locations were considered for leaves and rejected: they cost a
pointer in every node, and a file and line say less than a function name.

## Scope locals

`Scope` keeps a `fn(&L, &mut dyn Inspector)` to report its locals. A bound on
`L` would break manual scopes whose locals are not `Debug`. `scope!` generates
the function, reporting each local by name: `Debug` values through autoref
specialization, which resolves because the locals struct is concrete at
expansion, and `..` otherwise. An unset local reports `unset`.

`scope!` wraps its initializer sequence in a hidden `Inline`, which reports the
sequence's children in its place: initializers show as `name (compute)`
siblings of the body rather than under an extra `seq`.

## Text

`Describe` implements `Display`: one line with ` > ` for `{}`, indented lines
for `{:#}`, `.with_inactive()` for the whole definition with `*`/`-` markers.
Plumbing (`bind`, `no_params`) is transparent; decorators appear as nodes.
