//! Debug views of a tree and its invocation state.
//!
//! Every node reports itself through [`BtNode::inspect`]: what it is, its name,
//! whether it is on the running path, a few fields, and its children. An
//! [`Inspector`] receives that; [`Describe`] is the one that writes text.
//!
//! ```
//! use flatbt::prelude::*;
//!
//! fn has_ammo(ammo: &u32) -> bool {
//!     *ammo > 0
//! }
//!
//! let tree = select((
//!     seq((check(has_ammo), leaf(|_: &mut u32| NodeResult::Running("fire")))).named("attack"),
//!     leaf(|_: &mut u32| NodeResult::Running("reload")).named("reload"),
//! ));
//! let mut state = BtState::new(&tree);
//! let _ = update(&tree, &mut state, &mut 3, EntryMode::Evaluate);
//! assert_eq!(state.describe().to_string(), "select > attack (seq) > leaf");
//! assert_eq!(
//!     format!("{:#}", state.describe().with_inactive()),
//!     "* select\n\
//!      \x20 * attack (seq)\n\
//!      \x20   - has_ammo (check)\n\
//!      \x20   * leaf\n\
//!      \x20 - reload (leaf)",
//! );
//! ```
//!
//! Names come from [`named`] or `.named(..)`, and otherwise from code where
//! it has one: a function item passed to `leaf`, `check` or `guard`, an action
//! type, a custom node type, a `scope!` local, a `choose!` pattern. Closures
//! have none. Nothing here runs during [`update`](crate::update).

use core::fmt;
use core::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult};

/// One node, as reported to an [`Inspector`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeInfo<'a> {
    /// What the node is: its constructor, such as `seq` or `leaf`, or for a
    /// custom node its type name.
    pub kind: &'a str,
    /// Given with [`named`](WithName::named), or taken from code.
    pub name: Option<&'a str>,
    /// What the parent calls this child, such as a `choose!` pattern.
    pub label: Option<&'a str>,
    /// Holds invocation state: the node is on the running path.
    pub active: bool,
}

impl<'a> NodeInfo<'a> {
    pub fn new(kind: &'a str, active: bool) -> Self {
        Self {
            kind,
            name: None,
            label: None,
            active,
        }
    }

    pub fn name(self, name: Option<&'a str>) -> Self {
        Self { name, ..self }
    }
}

/// Receives a tree as nodes, fields, and nesting.
///
/// Each node is one [`enter`](Self::enter), then its fields, then its
/// children, then one [`exit`](Self::exit) -- unless `enter` returns false,
/// which skips the rest. Nodes report through [`node`](dyn Inspector::node),
/// which keeps that order.
pub trait Inspector {
    /// Starts a node. Returns whether to receive its fields and children.
    fn enter(&mut self, node: NodeInfo<'_>) -> bool;

    /// A value of the node last entered: configuration, or invocation state.
    fn field(&mut self, name: &str, value: &dyn fmt::Debug);

    /// Ends the node last entered.
    fn exit(&mut self);
}

impl dyn Inspector + '_ {
    /// Reports a node: `body` adds its fields, then its children.
    pub fn node(&mut self, node: NodeInfo<'_>, body: impl FnOnce(&mut dyn Inspector)) {
        if self.enter(node) {
            body(self);
            self.exit();
        }
    }
}

/// A node with a name for [`Inspector`]s. It runs exactly as `node` does.
pub struct Named<N> {
    node: N,
    name: &'static str,
    label: bool,
}

/// Names `node` in debug views, replacing any name taken from code. The same
/// as `node.named(name)`, with the name before a long node rather than after.
pub fn named<N>(name: &'static str, node: N) -> Named<N> {
    node.named(name)
}

/// Adds `node.named(name)`; see [`named`].
pub trait WithName: Sized {
    /// Names this node in debug views, replacing any name taken from code.
    fn named(self, name: &'static str) -> Named<Self> {
        Named {
            node: self,
            name,
            label: false,
        }
    }
}

impl<N> WithName for N {}

/// Marks `node` with what its parent calls it, such as the pattern that selects
/// it; `choose!` and `per_child!` do this for each arm. It runs exactly as
/// `node` does.
pub fn label<N>(label: &'static str, node: N) -> Named<N> {
    Named {
        node,
        name: label,
        label: true,
    }
}

impl<C, A, P, N: BtNode<C, A, P>> BtNode<C, A, P> for Named<N> {
    type State = N::State;

    #[inline(always)]
    fn update(
        &self,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A> {
        self.node.update(state, ctx, params, mode)
    }

    fn inspect(&self, state: Option<&N::State>, inspector: &mut dyn Inspector) {
        self.node.inspect(
            state,
            &mut Rename {
                inner: inspector,
                name: Some(self.name),
                label: self.label,
            },
        );
    }
}

/// Renames the next node entered.
struct Rename<'a, 'b> {
    inner: &'a mut (dyn Inspector + 'b),
    name: Option<&'a str>,
    label: bool,
}

impl Inspector for Rename<'_, '_> {
    fn enter(&mut self, node: NodeInfo<'_>) -> bool {
        match self.name.take() {
            Some(label) if self.label => self.inner.enter(NodeInfo {
                label: Some(label),
                ..node
            }),
            Some(name) => self.inner.enter(NodeInfo {
                name: Some(name),
                ..node
            }),
            None => self.inner.enter(node),
        }
    }

    fn field(&mut self, name: &str, value: &dyn fmt::Debug) {
        self.inner.field(name, value);
    }

    fn exit(&mut self) {
        self.inner.exit();
    }
}

/// The last path segment of `T`'s name, without generic arguments:
/// `ActionNode` for `flatbt::nodes::action::ActionNode<game::Walk>`.
pub fn type_label<T: ?Sized>() -> &'static str {
    last_segment(core::any::type_name::<T>())
}

/// The name of a function item, such as `has_ammo` in `check(has_ammo)`.
/// `None` for closures, function pointers, and other types.
pub fn fn_name<F>() -> Option<&'static str> {
    let full = core::any::type_name::<F>();
    let identifier = |text: &str| {
        text.starts_with(|c: char| c.is_alphabetic() || c == '_')
            && text.chars().all(|c| c.is_alphanumeric() || c == '_')
    };
    let name = last_segment(full);
    (!full.starts_with("fn(") && identifier(name)).then_some(name)
}

fn last_segment(full: &str) -> &str {
    let bytes = full.as_bytes();
    let (mut depth, mut start, mut end) = (0usize, 0, None);
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'<' | b'(' | b'[' => {
                if depth == 0 && end.is_none() {
                    end = Some(index);
                }
                depth += 1;
            }
            b'>' | b')' | b']' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && bytes.get(index + 1) == Some(&b':') => {
                index += 1;
                start = index + 1;
                end = None;
            }
            _ => {}
        }
        index += 1;
    }
    let segment = &full[start..end.unwrap_or(full.len())];
    if segment.is_empty() { full } else { segment }
}

/// A text view of a tree and its invocation state, from
/// [`BtState::describe`](crate::BtState::describe) or [`describe`].
///
/// `{}` writes the running path on one line, root first:
/// `select > attack (seq) > fire (leaf)`. `{:#}` writes one node per line,
/// indented by depth. Each node is `name (kind)`, or its kind when it has no
/// name; a label comes first, as `pattern => `, and fields follow in braces,
/// as `scope {target: 3}`. A tree that is not running writes `not running`.
pub struct Describe<'a, N: BtNode<C, A>, C, A = ()> {
    node: &'a N,
    state: Option<&'a N::State>,
    inactive: bool,
    context: PhantomData<fn(&mut C) -> A>,
}

/// A fingerprint of the running path: equal while the same nodes run, and in
/// practice different when any node enters or leaves it. Field values are left
/// out, so a scope local changing does not change it.
///
/// For logging only on change: keep the last id, and write
/// [`describe`] when a new one differs. Compare ids of one tree only.
///
/// ```
/// use flatbt::prelude::*;
///
/// let tree = select((
///     guard(|n: &u32| *n < 2, leaf(|_: &mut u32| NodeResult::RUNNING)),
///     leaf(|_: &mut u32| NodeResult::RUNNING),
/// ));
/// let mut state = BtState::new(&tree);
/// let mut n = 0;
/// let mut logged = Vec::new();
/// let mut last = None;
/// for _ in 0..4 {
///     let _ = update(&tree, &mut state, &mut n, EntryMode::Evaluate);
///     if last.replace(state.path_id()) != Some(state.path_id()) {
///         logged.push(state.describe().to_string());
///     }
///     n += 1;
/// }
/// assert_eq!(logged, ["select > guard > leaf", "select > leaf"]);
/// ```
pub fn path_id<C, A, N: BtNode<C, A>>(node: &N, state: Option<&N::State>) -> u64 {
    let mut hash = PathHash(0xcbf2_9ce4_8422_2325);
    node.inspect(state, &mut hash);
    hash.0
}

/// FNV-1a over the path's structure: which nodes are entered, in order.
/// Inactive siblings count too, so the position of an active child does.
struct PathHash(u64);

impl PathHash {
    fn feed(&mut self, byte: u8) {
        self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
}

impl Inspector for PathHash {
    fn enter(&mut self, node: NodeInfo<'_>) -> bool {
        self.feed(if node.active { 1 } else { 2 });
        node.active
    }

    fn field(&mut self, _: &str, _: &dyn fmt::Debug) {}

    fn exit(&mut self) {
        self.feed(3);
    }
}

/// Describes `node` over `state`, for a driver holding state without a
/// [`BtState`](crate::BtState), such as the slot [`update_slot`](crate::update_slot)
/// takes.
pub fn describe<'a, C, A, N: BtNode<C, A>>(
    node: &'a N,
    state: Option<&'a N::State>,
) -> Describe<'a, N, C, A> {
    Describe {
        node,
        state,
        inactive: false,
        context: PhantomData,
    }
}

impl<N: BtNode<C, A>, C, A> Describe<'_, N, C, A> {
    /// Also writes nodes off the running path, in `{:#}`. Each line then starts
    /// with `*` for a node on the path, or `-` for one off it.
    pub fn with_inactive(self) -> Self {
        Self {
            inactive: true,
            ..self
        }
    }
}

impl<N: BtNode<C, A>, C, A> fmt::Display for Describe<'_, N, C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines = f.alternate();
        let mut text = Text {
            f,
            lines,
            inactive: self.inactive && lines,
            depth: 0,
            nodes: 0,
            fields: false,
            result: Ok(()),
        };
        self.node.inspect(self.state, &mut text);
        text.close_fields();
        if text.nodes == 0 {
            text.write(format_args!("not running"));
        }
        text.result
    }
}

impl<N: BtNode<C, A>, C, A> fmt::Debug for Describe<'_, N, C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

struct Text<'a, 'f> {
    f: &'a mut fmt::Formatter<'f>,
    lines: bool,
    inactive: bool,
    depth: usize,
    nodes: usize,
    fields: bool,
    result: fmt::Result,
}

impl Text<'_, '_> {
    fn write(&mut self, args: fmt::Arguments<'_>) {
        if self.result.is_ok() {
            self.result = self.f.write_fmt(args);
        }
    }

    fn close_fields(&mut self) {
        if core::mem::take(&mut self.fields) {
            self.write(format_args!("}}"));
        }
    }
}

impl Inspector for Text<'_, '_> {
    fn enter(&mut self, node: NodeInfo<'_>) -> bool {
        if !node.active && !self.inactive {
            return false;
        }
        self.close_fields();
        if self.lines {
            if self.nodes > 0 {
                self.write(format_args!("\n"));
            }
            self.write(format_args!("{:1$}", "", self.depth * 2));
            if self.inactive {
                self.write(format_args!("{} ", if node.active { '*' } else { '-' }));
            }
        } else if self.nodes > 0 {
            self.write(format_args!(" > "));
        }
        if let Some(label) = node.label {
            self.write(format_args!("{label} => "));
        }
        match node.name {
            Some(name) => self.write(format_args!("{name} ({})", node.kind)),
            None => self.write(format_args!("{}", node.kind)),
        }
        self.depth += 1;
        self.nodes += 1;
        true
    }

    fn field(&mut self, name: &str, value: &dyn fmt::Debug) {
        let open = if core::mem::replace(&mut self.fields, true) {
            ", "
        } else {
            " {"
        };
        self.write(format_args!("{open}{name}: {value:?}"));
    }

    fn exit(&mut self) {
        self.close_fields();
        self.depth -= 1;
    }
}

/// Used by `scope!`: its initializer sequence, and locals reported whether or
/// not their types implement `Debug`.
#[doc(hidden)]
pub mod __private {
    use core::fmt;

    use super::{Inspector, NodeInfo};
    use crate::{BtNode, EntryMode, NodeResult};

    /// Runs `N`; reports its children in its place, as children of the parent.
    pub struct Inline<N>(pub N);

    impl<C, A, P, N: BtNode<C, A, P>> BtNode<C, A, P> for Inline<N> {
        type State = N::State;

        #[inline(always)]
        fn update(
            &self,
            state: &mut N::State,
            ctx: &mut C,
            params: P,
            mode: EntryMode,
        ) -> NodeResult<A> {
            self.0.update(state, ctx, params, mode)
        }

        fn inspect(&self, state: Option<&N::State>, inspector: &mut dyn Inspector) {
            self.0.inspect(
                state,
                &mut Skip {
                    inner: inspector,
                    depth: 0,
                },
            );
        }
    }

    /// Drops the first node entered, keeping its descendants.
    struct Skip<'a, 'b> {
        inner: &'a mut (dyn Inspector + 'b),
        depth: usize,
    }

    impl Inspector for Skip<'_, '_> {
        fn enter(&mut self, node: NodeInfo<'_>) -> bool {
            if self.depth == 0 {
                self.depth = 1;
                return true;
            }
            let entered = self.inner.enter(node);
            self.depth += usize::from(entered);
            entered
        }

        fn field(&mut self, name: &str, value: &dyn fmt::Debug) {
            if self.depth > 1 {
                self.inner.field(name, value);
            }
        }

        fn exit(&mut self) {
            if self.depth > 1 {
                self.inner.exit();
            }
            self.depth -= 1;
        }
    }

    pub struct Probe<'a, T>(pub &'a T);

    pub trait ViaDebug<'a> {
        fn probe(&self) -> &'a dyn fmt::Debug;
    }

    impl<'a, T: fmt::Debug> ViaDebug<'a> for Probe<'a, T> {
        fn probe(&self) -> &'a dyn fmt::Debug {
            self.0
        }
    }

    pub trait ViaOpaque<'a> {
        fn probe(&self) -> &'a dyn fmt::Debug;
    }

    impl<'a, T> ViaOpaque<'a> for &Probe<'a, T> {
        fn probe(&self) -> &'a dyn fmt::Debug {
            &Opaque
        }
    }

    struct Opaque;

    impl fmt::Debug for Opaque {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("..")
        }
    }

    pub fn local(inspector: &mut dyn Inspector, name: &str, value: Option<&dyn fmt::Debug>) {
        match value {
            Some(value) => inspector.field(name, value),
            None => inspector.field(name, &format_args!("unset")),
        }
    }
}
