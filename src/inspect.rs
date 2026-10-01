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

use crate::{BtNode, Entry, NodeResult};

/// One node, as reported to an [`Inspector`].
///
/// Names taken from code, such as a function item's, are resolved only when
/// read: an inspector that never asks for them, like [`path_id`], never pays
/// for them.
#[derive(Clone, Copy)]
pub struct NodeInfo<'a> {
    kind: Kind<'a>,
    name: Name<'a>,
    label: Option<&'a str>,
    active: bool,
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Given(&'a str),
    TypeOf(fn() -> &'static str),
}

#[derive(Clone, Copy)]
enum Name<'a> {
    Given(Option<&'a str>),
    TypeOf(fn() -> &'static str),
    FnOf(fn() -> Option<&'static str>),
}

impl<'a> NodeInfo<'a> {
    /// A node of `kind`, such as `seq`; `active` when it is on the running path.
    pub fn new(kind: &'a str, active: bool) -> Self {
        Self {
            kind: Kind::Given(kind),
            name: Name::Given(None),
            label: None,
            active,
        }
    }

    /// A node whose kind is `Type`'s type label; see [`type_label`].
    pub fn of_type<Type: ?Sized>(active: bool) -> Self {
        Self {
            kind: Kind::TypeOf(type_label::<Type>),
            ..Self::new("", active)
        }
    }

    /// Names the node.
    pub fn with_name(self, name: Option<&'a str>) -> Self {
        Self {
            name: Name::Given(name),
            ..self
        }
    }

    /// Names the node after the function item `Function`, if it is one; see
    /// [`fn_name`].
    pub fn with_fn_name<Function>(self) -> Self {
        Self {
            name: Name::FnOf(fn_name::<Function>),
            ..self
        }
    }

    /// Names the node after the type `Type`; see [`type_label`].
    pub fn with_type_name<Type: ?Sized>(self) -> Self {
        Self {
            name: Name::TypeOf(type_label::<Type>),
            ..self
        }
    }

    /// What the node is: its constructor, such as `seq` or `leaf`, or for a
    /// custom node its type name.
    pub fn kind(&self) -> &'a str {
        match self.kind {
            Kind::Given(kind) => kind,
            Kind::TypeOf(kind) => kind(),
        }
    }

    /// Given with [`named`](WithName::named), or taken from code.
    pub fn name(&self) -> Option<&'a str> {
        match self.name {
            Name::Given(name) => name,
            Name::TypeOf(name) => Some(name()),
            Name::FnOf(name) => name(),
        }
    }

    /// What the parent calls this child, such as a `choose!` pattern.
    pub fn label(&self) -> Option<&'a str> {
        self.label
    }

    /// Holds invocation state: the node is on the running path.
    pub fn active(&self) -> bool {
        self.active
    }
}

impl fmt::Debug for NodeInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeInfo")
            .field("kind", &self.kind())
            .field("name", &self.name())
            .field("label", &self.label)
            .field("active", &self.active)
            .finish()
    }
}

impl PartialEq for NodeInfo<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind()
            && self.name() == other.name()
            && self.label == other.label
            && self.active == other.active
    }
}

impl Eq for NodeInfo<'_> {}

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
pub struct Named<Node> {
    node: Node,
    name: &'static str,
    label: bool,
}

/// Names `node` in debug views, replacing any name taken from code. The same
/// as `node.named(name)`, with the name before a long node rather than after.
pub fn named<Node>(name: &'static str, node: Node) -> Named<Node> {
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

impl<Node> WithName for Node {}

/// Marks `node` with what its parent calls it, such as the pattern that selects
/// it; `choose!` and `per_child!` do this for each arm. It runs exactly as
/// `node` does.
pub fn label<Node>(label: &'static str, node: Node) -> Named<Node> {
    Named {
        node,
        name: label,
        label: true,
    }
}

impl<Context, Act, Params, Node: BtNode<Context, Act, Params>> BtNode<Context, Act, Params>
    for Named<Node>
{
    type State = Node::State;
    type Memory = Node::Memory;
    const NODES: usize = Node::NODES;

    #[inline(always)]
    fn update(
        &self,
        state: &mut Node::State,
        memory: &mut Node::Memory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        self.node.update(state, memory, ctx, params, entry)
    }

    fn inspect(
        &self,
        state: Option<&Node::State>,
        memory: &Node::Memory,
        inspector: &mut dyn Inspector,
    ) {
        self.node.inspect(
            state,
            memory,
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
            Some(name) => self.inner.enter(node.with_name(Some(name))),
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

/// The last path segment of `Type`'s name, without generic arguments:
/// `ActionNode` for `flatbt::nodes::action::ActionNode<game::Walk>`.
pub fn type_label<Type: ?Sized>() -> &'static str {
    last_segment(core::any::type_name::<Type>())
}

/// The name of a function item, such as `has_ammo` in `check(has_ammo)`.
/// `None` for closures, function pointers, and other types.
pub fn fn_name<Function>() -> Option<&'static str> {
    let full = core::any::type_name::<Function>();
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
pub struct Describe<'a, Node: BtNode<Context, Act>, Context, Act = ()> {
    node: &'a Node,
    state: Option<&'a Node::State>,
    memory: &'a Node::Memory,
    inactive: bool,
    context: PhantomData<fn(&mut Context) -> Act>,
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
pub fn path_id<Context, Act, Node: BtNode<Context, Act>>(
    node: &Node,
    state: Option<&Node::State>,
    memory: &Node::Memory,
) -> u64 {
    let mut hash = PathHash(0xcbf2_9ce4_8422_2325);
    node.inspect(state, memory, &mut hash);
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
        self.feed(if node.active() { 1 } else { 2 });
        node.active()
    }

    fn field(&mut self, _: &str, _: &dyn fmt::Debug) {}

    fn exit(&mut self) {
        self.feed(3);
    }
}

/// Describes `node` over `state` and `memory`, for a driver holding them
/// without a [`BtState`](crate::BtState), such as the slots
/// [`update_slot`](crate::update_slot) takes.
pub fn describe<'a, Context, Act, Node: BtNode<Context, Act>>(
    node: &'a Node,
    state: Option<&'a Node::State>,
    memory: &'a Node::Memory,
) -> Describe<'a, Node, Context, Act> {
    Describe {
        node,
        state,
        memory,
        inactive: false,
        context: PhantomData,
    }
}

impl<Node: BtNode<Context, Act>, Context, Act> Describe<'_, Node, Context, Act> {
    /// Also writes nodes off the running path, in `{:#}`. Each line then starts
    /// with `*` for a node on the path, or `-` for one off it.
    pub fn with_inactive(self) -> Self {
        Self {
            inactive: true,
            ..self
        }
    }
}

impl<Node: BtNode<Context, Act>, Context, Act> fmt::Display for Describe<'_, Node, Context, Act> {
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
        self.node.inspect(self.state, self.memory, &mut text);
        text.close_fields();
        if text.nodes == 0 {
            text.write(format_args!("not running"));
        }
        text.result
    }
}

impl<Node: BtNode<Context, Act>, Context, Act> fmt::Debug for Describe<'_, Node, Context, Act> {
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
        if !node.active() && !self.inactive {
            return false;
        }
        self.close_fields();
        if self.lines {
            if self.nodes > 0 {
                self.write(format_args!("\n"));
            }
            self.write(format_args!("{:1$}", "", self.depth * 2));
            if self.inactive {
                self.write(format_args!("{} ", if node.active() { '*' } else { '-' }));
            }
        } else if self.nodes > 0 {
            self.write(format_args!(" > "));
        }
        if let Some(label) = node.label() {
            self.write(format_args!("{label} => "));
        }
        match node.name() {
            Some(name) => self.write(format_args!("{name} ({})", node.kind())),
            None => self.write(format_args!("{}", node.kind())),
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

/// Used by `scope!` to report locals whether or not their types implement
/// `Debug`.
#[doc(hidden)]
pub mod __private {
    use core::fmt;

    use super::Inspector;

    pub struct Probe<'a, Value>(pub &'a Value);

    pub trait ViaDebug<'a> {
        fn probe(&self) -> &'a dyn fmt::Debug;
    }

    impl<'a, Value: fmt::Debug> ViaDebug<'a> for Probe<'a, Value> {
        fn probe(&self) -> &'a dyn fmt::Debug {
            self.0
        }
    }

    pub trait ViaOpaque<'a> {
        fn probe(&self) -> &'a dyn fmt::Debug;
    }

    impl<'a, Value> ViaOpaque<'a> for &Probe<'a, Value> {
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
