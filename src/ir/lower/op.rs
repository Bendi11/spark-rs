use codespan_reporting::diagnostic::{Diagnostic, Label};

use crate::{ir::{value::{IrExpr, IrExprKind}, FunId, BBId, types::IrType, IrContext}, util::{files::FileId, loc::Span}, ast::Expr, parse::token::Op};

use super::{IrLowerer, IntermediateModuleId};


impl<'files, 'ctx> IrLowerer<'files, 'ctx> {
    /// Lower a binary expression to IR
    pub fn lower_bin(
        &mut self,
        module: IntermediateModuleId,
        file: FileId,
        fun: FunId,
        lhs: &Expr,
        op: Op,
        rhs: &Expr,
        bb: BBId,
    ) -> Result<IrExpr, Diagnostic<FileId>> {
        let lhs = self.lower_expr(module, file, fun, lhs, bb)?;
        let rhs = self.lower_expr(module, file, fun, rhs, bb)?;

        let ty = match (&self.ctx[lhs.ty], op, &self.ctx[rhs.ty]) {
            (IrType::Bool, Op::LogicalAnd | Op::LogicalOr | Op::LogicalNot | Op::Eq, IrType::Bool) => IrContext::BOOL,
            (
                IrType::Integer(_),
                Op::Eq| Op::Greater | Op::GreaterEq | Op::Less | Op::LessEq | Op::Star | Op::Div | Op::Add | Op::Sub | Op::ShLeft | Op::ShRight,
                IrType::Integer(_),
            ) => lhs.ty,
            (
                IrType::Float(_),
                Op::Eq | Op::Greater | Op::GreaterEq | Op::Less | Op::LessEq | Op::Star | Op::Div | Op::Add | Op::Sub,
                IrType::Float(_)
            ) => lhs.ty,
            (
                IrType::Ptr(_),
                Op::ShRight | Op::ShLeft,
                IrType::Integer(_)
            ) => lhs.ty,
            (
                IrType::Ptr(_),
                Op::Add | Op::Sub,
                IrType::Ptr(_) | IrType::Integer(_)
            ) => lhs.ty,
            _ => return Err(Diagnostic::error()
                .with_message(format!(
                    "Cannot apply binary operator {} to operand types {} and {}",
                    op,
                    self.ctx.typename(lhs.ty),
                    self.ctx.typename(rhs.ty),
                ))
                .with_labels(vec![
                    Label::primary(file, Span::from(lhs.span.from..rhs.span.to)),
                    Label::secondary(file, lhs.span)
                        .with_message(format!("LHS of type {} appears here", self.ctx.typename(lhs.ty))),
                    Label::secondary(file, rhs.span)
                        .with_message(format!("RHS of type {} appears here", self.ctx.typename(rhs.ty))),
                ])
            )
        };

        Ok(IrExpr {
            span: (lhs.span.from..rhs.span.to).into(),
            ty,
            kind: IrExprKind::Binary(Box::new(lhs), op, Box::new(rhs)),
        })
    }
    
    /// Lower a unary expression to IR
    pub fn lower_unary(
        &mut self,
        module: IntermediateModuleId,
        file: FileId,
        fun: FunId,
        op: Op,
        expr: &Expr,
        bb: BBId,
    ) -> Result<IrExpr, Diagnostic<FileId>> {
        let expr = self.lower_expr(module, file, fun, expr, bb)?;

        let ty = match (op, self.ctx[expr.ty].clone()) {
            (Op::Star, IrType::Ptr(to)) => to,
            (Op::AND, _) => self.ctx.types.insert(IrType::Ptr(expr.ty)),
            (Op::Sub, IrType::Integer(_) | IrType::Float(_)) => expr.ty,
            (Op::NOT, IrType::Integer(_) | IrType::Ptr(_)) => expr.ty,
            _ => return Err(Diagnostic::error()
                .with_message(format!(
                    "Cannot apply unary operator {} to expression of type {}",
                    op,
                    self.ctx.typename(expr.ty),
                ))
                .with_labels(vec![
                    Label::primary(file, expr.span),
                ])
            )
        };

        Ok(IrExpr {
            ty,
            span: expr.span,
            kind: IrExprKind::Unary(op, Box::new(expr)),
        })
    }
}