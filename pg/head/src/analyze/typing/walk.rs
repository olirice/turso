use super::Typed;

pub(crate) trait TypedVisitor {
    type Stop;

    fn enter(&mut self, typed: &Typed) -> Result<(), Self::Stop>;
}

pub(crate) fn walk_typed<V: TypedVisitor + ?Sized>(
    visitor: &mut V,
    typed: &Typed,
) -> Result<(), V::Stop> {
    match typed {
        Typed::Column(..)
        | Typed::TableOid(..)
        | Typed::Value(..)
        | Typed::CurrentUser
        | Typed::AlwaysFalse
        | Typed::Subquery(_)
        | Typed::Exists(_)
        | Typed::InSelect(..)
        | Typed::ArrayAgg(..) => Ok(()),
        Typed::Cast(inner, _)
        | Typed::Not(inner)
        | Typed::IsNull(inner, _)
        | Typed::Is(inner, _, _) => visitor.enter(inner),
        Typed::And(items) | Typed::Or(items) => {
            items.iter().try_for_each(|item| visitor.enter(item))
        }
        Typed::Compare(_, left, right)
        | Typed::DistinctFrom(left, right, _)
        | Typed::Concat(left, right) => {
            visitor.enter(left)?;
            visitor.enter(right)
        }
        Typed::In(needle, list, _) => {
            visitor.enter(needle)?;
            list.iter().try_for_each(|item| visitor.enter(item))
        }
        Typed::Case {
            base,
            arms,
            otherwise,
            ..
        } => {
            if let Some(base) = base {
                visitor.enter(base)?;
            }
            for (when, then) in arms {
                visitor.enter(when)?;
                visitor.enter(then)?;
            }
            if let Some(otherwise) = otherwise {
                visitor.enter(otherwise)?;
            }
            Ok(())
        }
        Typed::Call(_, args) | Typed::ArrayLiteral(args, _) => {
            args.iter().try_for_each(|arg| visitor.enter(arg))
        }
        Typed::Subscript(base, index, _, _) => {
            visitor.enter(base)?;
            visitor.enter(index)
        }
        Typed::AnyEq(needle, array) => {
            visitor.enter(needle)?;
            visitor.enter(array)
        }
    }
}

pub(crate) trait TypedMap {
    type Error;

    fn enter(&mut self, typed: Typed) -> Result<Typed, Self::Error>;
}

pub(crate) fn walk_typed_map<M: TypedMap + ?Sized>(
    mapper: &mut M,
    typed: Typed,
) -> Result<Typed, M::Error> {
    Ok(match typed {
        leaf @ (Typed::Column(..)
        | Typed::TableOid(..)
        | Typed::Value(..)
        | Typed::CurrentUser
        | Typed::AlwaysFalse) => leaf,
        Typed::Cast(inner, ty) => Typed::Cast(Box::new(mapper.enter(*inner)?), ty),
        Typed::Not(inner) => Typed::Not(Box::new(mapper.enter(*inner)?)),
        Typed::And(items) => Typed::And(map_all(mapper, items)?),
        Typed::Or(items) => Typed::Or(map_all(mapper, items)?),
        Typed::Compare(op, left, right) => Typed::Compare(
            op,
            Box::new(mapper.enter(*left)?),
            Box::new(mapper.enter(*right)?),
        ),
        Typed::IsNull(inner, negated) => Typed::IsNull(Box::new(mapper.enter(*inner)?), negated),
        Typed::Is(inner, truth, negated) => {
            Typed::Is(Box::new(mapper.enter(*inner)?), truth, negated)
        }
        Typed::DistinctFrom(left, right, negated) => Typed::DistinctFrom(
            Box::new(mapper.enter(*left)?),
            Box::new(mapper.enter(*right)?),
            negated,
        ),
        Typed::In(needle, list, negated) => Typed::In(
            Box::new(mapper.enter(*needle)?),
            map_all(mapper, list)?,
            negated,
        ),
        Typed::Concat(left, right) => Typed::Concat(
            Box::new(mapper.enter(*left)?),
            Box::new(mapper.enter(*right)?),
        ),
        Typed::Case {
            base,
            arms,
            otherwise,
            result,
        } => Typed::Case {
            base: base
                .map(|base| mapper.enter(*base))
                .transpose()?
                .map(Box::new),
            arms: arms
                .into_iter()
                .map(|(when, then)| Ok((mapper.enter(when)?, mapper.enter(then)?)))
                .collect::<Result<Vec<_>, M::Error>>()?,
            otherwise: otherwise
                .map(|otherwise| mapper.enter(*otherwise))
                .transpose()?
                .map(Box::new),
            result,
        },
        leaf @ (Typed::Subquery(_) | Typed::Exists(_)) => leaf,
        Typed::InSelect(needle, query, negated) => {
            Typed::InSelect(Box::new(mapper.enter(*needle)?), query, negated)
        }
        Typed::Call(handle, args) => Typed::Call(handle, map_all(mapper, args)?),
        Typed::ArrayLiteral(elements, ty) => Typed::ArrayLiteral(map_all(mapper, elements)?, ty),
        Typed::Subscript(base, index, ty, zero_based) => Typed::Subscript(
            Box::new(mapper.enter(*base)?),
            Box::new(mapper.enter(*index)?),
            ty,
            zero_based,
        ),
        Typed::AnyEq(needle, array) => Typed::AnyEq(
            Box::new(mapper.enter(*needle)?),
            Box::new(mapper.enter(*array)?),
        ),
        leaf @ Typed::ArrayAgg(..) => leaf,
    })
}

fn map_all<M: TypedMap + ?Sized>(
    mapper: &mut M,
    items: Vec<Typed>,
) -> Result<Vec<Typed>, M::Error> {
    items.into_iter().map(|item| mapper.enter(item)).collect()
}
