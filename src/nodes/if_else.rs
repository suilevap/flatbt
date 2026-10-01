use crate::inspect::{Inspector, fn_name};
use crate::{BtControl, ControlNode, ControlOp, control};

/// Runs one of two children by a condition.
pub struct IfElse<Condition>(Condition);

/// Runs `then` when `condition` holds, `otherwise` when it does not, and
/// returns that child's result without fallback.
///
/// Like [`choose!`](crate::choose) with two arms: Evaluate asks again and may
/// switch branch, dropping the running one; Resume keeps the running branch.
///
/// ```
/// use flatbt::prelude::*;
///
/// let tree = if_else(
///     |ammo: &u32| *ammo > 0,
///     leaf(|_: &mut u32| NodeResult::Running("fire")),
///     leaf(|_: &mut u32| NodeResult::Running("reload")),
/// );
/// let mut state = BtState::new(&tree);
/// assert_eq!(update(&tree, &mut state, &mut 0, EntryMode::Evaluate).act(), Some("reload"));
/// ```
pub fn if_else<Condition, Then, Otherwise>(
    condition: Condition,
    then: Then,
    otherwise: Otherwise,
) -> ControlNode<IfElse<Condition>, (Then, Otherwise)> {
    control(IfElse(condition), (then, otherwise))
}

impl<Context, Condition: Fn(&Context) -> bool> BtControl<Context> for IfElse<Condition> {
    type State = ();
    type Memory = ();

    fn kind(&self) -> &'static str {
        "if_else"
    }

    fn inspect(&self, _: Option<&()>, _: &(), _: Option<usize>, inspector: &mut dyn Inspector) {
        if let Some(condition) = fn_name::<Condition>() {
            inspector.field("if", &format_args!("{condition}"));
        }
    }

    #[inline]
    fn begin(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        _: Option<usize>,
        _: usize,
    ) -> ControlOp {
        ControlOp::RunChild(if (self.0)(ctx) { 0 } else { 1 })
    }

    #[inline]
    fn child_succeeded(
        &self,
        _: &mut (),
        _: &mut (),
        _: &mut Context,
        _: usize,
        _: usize,
    ) -> ControlOp {
        ControlOp::Success
    }

    #[inline]
    fn child_failed(
        &self,
        _: &mut (),
        _: &mut (),
        _: &mut Context,
        _: usize,
        _: usize,
    ) -> ControlOp {
        ControlOp::Failure
    }
}
