//! Linking closed, typed raw Simplicity fragments into HL construction graphs.
use std::sync::Arc;

use simplicity::dag::{DagLike, InternalSharing, PostOrderIterItem};
use simplicity::node::{self, Converter, Inner, NoDisconnect, NoWitness, Node};
use simplicity::{types, CommitNode};

use crate::ast::JetHinter;
use crate::driver::SourceMap;
use crate::named::{ConstructNode, WithNames};
use crate::types::{ResolvedType, StructuralType};
use crate::TemplateProgramWitness;

pub(crate) fn load(
    path: &str,
    entry: &str,
    file_id: usize,
    sources: Option<&SourceMap>,
    jets: &dyn JetHinter,
    input: &ResolvedType,
    output: &ResolvedType,
) -> Result<Arc<CommitNode>, String> {
    let path = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        let declaring_file = sources.and_then(|s| s.path(file_id)).ok_or(
            "relative external paths require a named source file; use the file-based compiler API",
        )?;
        let parent = declaring_file
            .as_path()
            .parent()
            .ok_or("source file has no parent directory")?;
        parent.join(path)
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let forest = jets
        .parse_simplicity(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let root = forest
        .roots()
        .get(entry)
        .ok_or_else(|| format!("{} has no root entry `{entry}`", path.display()))?
        .to_commit_node();
    for item in root.as_ref().post_order_iter::<InternalSharing>() {
        match item.node.inner() {
            Inner::Witness(_) | Inner::Disconnect(..) => return Err(format!(
                "{}: entry `{entry}` contains a witness or disconnect; pass inputs through HL parameters",
                path.display()
            )),
            _ => {}
        }
    }
    let input = StructuralType::from(input);
    let output = StructuralType::from(output);
    if root.arrow().source.as_ref() != input.as_ref()
        || root.arrow().target.as_ref() != output.as_ref()
    {
        return Err(format!(
            "{}: entry `{entry}` has type {}, declared {input} → {output}",
            path.display(),
            root.arrow()
        ));
    }
    Ok(root)
}

/// The parser already checked every combinator and finalized its arrow. Copy
/// those exact arrows into the caller's context rather than re-inferring them:
/// re-inference would lose annotations on polymorphic combinators such as iden.
/// Internal sharing is preserved within each imported call.
pub(crate) fn import<'brand>(
    root: &CommitNode,
    ctx: &types::Context<'brand>,
) -> Arc<ConstructNode<'brand>> {
    struct Importer<'a, 'brand>(&'a types::Context<'brand>);
    impl<'brand> Converter<node::Commit, WithNames<node::Construct<'brand>>> for Importer<'_, 'brand> {
        type Error = std::convert::Infallible;
        fn convert_witness(
            &mut self,
            _: &PostOrderIterItem<&CommitNode>,
            _: &NoWitness,
        ) -> Result<TemplateProgramWitness, Self::Error> {
            unreachable!("raw witnesses are rejected during analysis")
        }
        fn convert_disconnect(
            &mut self,
            _: &PostOrderIterItem<&CommitNode>,
            _: Option<&Arc<ConstructNode<'brand>>>,
            _: &NoDisconnect,
        ) -> Result<NoDisconnect, Self::Error> {
            unreachable!("raw disconnects are rejected during analysis")
        }
        fn convert_data(
            &mut self,
            data: &PostOrderIterItem<&CommitNode>,
            _: Inner<
                &Arc<Node<WithNames<node::Construct<'brand>>>>,
                &NoDisconnect,
                &TemplateProgramWitness,
            >,
        ) -> Result<node::ConstructData<'brand>, Self::Error> {
            let arrow = data.node.arrow();
            Ok(node::ConstructData::new(types::arrow::Arrow {
                source: types::Type::complete(self.0, Arc::clone(&arrow.source)),
                target: types::Type::complete(self.0, Arc::clone(&arrow.target)),
                inference_context: self.0.shallow_clone(),
            }))
        }
    }
    match root.convert::<InternalSharing, _, _>(&mut Importer(ctx)) {
        Ok(node) => node,
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linking_preserves_typed_nodes_and_internal_sharing() {
        let forest = simplicity::human_encoding::Forest::parse::<simplicity::jet::Core>(
            "id := iden : 2^32 -> 2^32\nentry := pair id id : 2^32 -> (2^32 * 2^32)",
        )
        .unwrap();
        let raw = forest.roots()["entry"].to_commit_node();
        types::Context::with_context(|ctx| {
            let linked = import(&raw, &ctx);
            let original_nodes: Vec<_> =
                raw.as_ref().post_order_iter::<InternalSharing>().collect();
            let linked_nodes: Vec<_> = linked
                .as_ref()
                .post_order_iter::<InternalSharing>()
                .collect();
            assert_eq!(
                original_nodes.len(),
                2,
                "fixture must have a shared iden child"
            );
            assert_eq!(original_nodes.len(), linked_nodes.len());
            for (original, linked) in original_nodes.iter().zip(&linked_nodes) {
                assert_eq!(original.node.cmr(), linked.node.cmr());
                assert_eq!(
                    *original.node.arrow(),
                    linked.node.cached_data().arrow().finalize().unwrap()
                );
            }
            let finalized = crate::named::finalize_types(&linked).unwrap();
            assert_eq!(crate::named::forget_names(&finalized), raw);
        });
    }
}
