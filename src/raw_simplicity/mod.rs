mod load;
mod sources;

use std::path::Path;
use std::sync::Arc;

use simplicity::dag::{DagLike, InternalSharing};
use simplicity::node::Inner;
use simplicity::CommitNode;

use crate::ast::JetHinter;
use crate::error::Error;
use crate::parse::parse_simplicity;
use crate::types::{ResolvedType, StructuralType};

pub use self::load::load_raw_code;
pub use self::sources::RawSources;
pub(crate) use self::sources::{RawFile, RawFiles};

/// The root of a raw Simplicity program that an `extern fn` imports.
pub(crate) const EXPORT_ROOT: &str = "export";

/// The raw Simplicity code of an `extern fn`: the closed `export` root of a raw program.
///
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawCode {
    root: Arc<CommitNode>,
    /// Where the text came from, for error messages.
    origin: Arc<Path>,
}

impl RawCode {
    /// Parse the `export` root of `text` with the jet set of `jet_hinter`.
    ///
    /// ## Errors
    ///
    /// The text is not valid raw Simplicity, has no `export` root, or the root is not closed:
    /// it contains a witness or a disconnect.
    pub fn parse(text: &str, origin: Arc<Path>, jet_hinter: &dyn JetHinter) -> Result<Self, Error> {
        let raw_error = |msg: String| Error::Simplicity { msg };

        let root = parse_simplicity(text, jet_hinter)
            .map_err(|e| raw_error(format!("{}: {e}", origin.display())))?;
        let is_closed = root
            .as_ref()
            .post_order_iter::<InternalSharing>()
            .all(|item| !matches!(item.node.inner(), Inner::Witness(_) | Inner::Disconnect(..)));

        if !is_closed {
            return Err(raw_error(format!(
                "{}: entry `{EXPORT_ROOT}` contains a witness or disconnect; pass inputs through HL parameters",
                origin.display()
            )));
        }

        Ok(Self { root, origin })
    }

    /// The `export` root.
    pub(crate) fn root(&self) -> &Arc<CommitNode> {
        &self.root
    }

    /// Check that the root has the structure of the declared signature `input → output`.
    pub(crate) fn check_signature(
        &self,
        input: &ResolvedType,
        output: &ResolvedType,
    ) -> Result<(), Error> {
        let found_input = StructuralType::from(Arc::clone(&self.root.arrow().source));
        let found_output = StructuralType::from(Arc::clone(&self.root.arrow().target));
        if found_input != StructuralType::from(input)
            || found_output != StructuralType::from(output)
        {
            return Err(Error::SimplicityTypeMismatch {
                origin: self.origin.clone(),
                entry: EXPORT_ROOT.to_owned(),
                expected_input: Box::new(input.clone()),
                expected_output: Box::new(output.clone()),
                found_input: Box::new(found_input),
                found_output: Box::new(found_output),
            });
        }
        Ok(())
    }
}
