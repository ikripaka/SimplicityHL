use std::sync::Arc;

use simplicity::dag::{InternalSharing, PostOrderIterItem};
use simplicity::node::{self, Converter, Inner, NoDisconnect, NoWitness, Node};
use simplicity::{types, CommitNode};

use crate::named::{ConstructNode, WithNames};
use crate::TemplateProgramWitness;

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

    // Converts Simplicity node into a CommitNode for SimplicityHL
    match root.convert::<InternalSharing, _, _>(&mut Importer(ctx)) {
        Ok(node) => node,
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::CoreJetHinter;
    use crate::parse::parse_simplicity;
    use simplicity::dag::DagLike;

    #[test]
    fn linking_preserves_typed_nodes_and_internal_sharing() {
        let raw = parse_simplicity(
            "id := iden : 2^32 -> 2^32\n\
             export := pair id id : 2^32 -> (2^32 * 2^32)",
            &CoreJetHinter,
        )
        .unwrap();

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
