//! `Array.wrap(x)` lowering: ActiveSupport's class-level `Array.wrap`
//! ships as a reopen of `Array`'s singleton class — a core_ext no
//! transpiled runtime can host, same as `symbolize_keys`,
//! `presence_in`, and `index_by` in this file's family. Rewrites
//! `Array.wrap(x)` to `ActiveSupport.wrap(x)`
//! (`runtime/ruby/active_support_ext.rb`): a receiver-shape rewrite of
//! a name no other pass produces or consumes, so no ordering
//! constraints.
//!
//! Unlike `symbolize_keys`, the receiver here is a bare `Array` Const,
//! not a typed value — every syntactic `Array.wrap(x)` rewrites, with
//! no receiver-type branch and no evaluation-order question (the sole
//! argument passes straight through, read exactly once either way).
//! The analyzer (`src/analyze/body/send.rs`'s `dispatch`) has already
//! typed the call's *result* as `Array[elem]` before this pass runs;
//! the rewrite changes only what gets emitted, not what the call means.

use crate::app::App;
use crate::expr::{Expr, ExprNode};

pub fn apply_array_wrap_lowering(app: &mut App) {
    super::for_each_hook_body(app, &mut rewrite);
    for view in &mut app.views {
        rewrite(&mut view.body);
    }
}

fn rewrite(expr: &mut Expr) {
    expr.node.for_each_child_mut(&mut rewrite);
    let replacement = match &mut *expr.node {
        ExprNode::Send { recv: Some(r), method, args, block: None, .. }
            if method.as_str() == "wrap"
                && args.len() == 1
                && matches!(
                    &*r.node,
                    ExprNode::Const { path } if path.len() == 1 && path[0].as_str() == "Array"
                ) =>
        {
            let ty = expr.ty.clone();
            let mut call = Expr::new(
                expr.span,
                ExprNode::Send {
                    recv: Some(Expr::new(
                        expr.span,
                        ExprNode::Const { path: vec![crate::ident::Symbol::from("ActiveSupport")] },
                    )),
                    method: crate::ident::Symbol::from("wrap"),
                    args: args.clone(),
                    block: None,
                    parenthesized: true,
                },
            );
            call.ty = ty;
            Some(call)
        }
        _ => None,
    };
    if let Some(r) = replacement {
        *expr = r;
    }
}
