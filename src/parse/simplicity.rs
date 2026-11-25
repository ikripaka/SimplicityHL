use std::sync::Arc;

use simplicity::human_encoding::Forest;
use simplicity::jet::Jet;
use simplicity::CommitNode;

use crate::ast::JetHinter;
use crate::raw_simplicity::EXPORT_ROOT;

/// Parses the [`EXPORT_ROOT`](crate::raw_simplicity::EXPORT_ROOT) root of a raw Simplicity file with the jet set of `self`.
///
/// Every [`JetHinter`](crate::ast::JetHinter) implements this trait. By default, a jet set has no
/// raw parser; the jet sets of this crate redefine it to parse with their own jets.
pub trait SimplicityParser {
    fn parse_simplicity(&self, _source: &str) -> Result<Arc<CommitNode>, String> {
        Err("raw Simplicity parsing is not supported for this jet set".to_owned())
    }
}

/// Parses the `export` root of a raw Simplicity file with the jet set of `jet_hinter`.
pub fn parse_simplicity(
    source: &str,
    jet_hinter: &dyn JetHinter,
) -> Result<Arc<CommitNode>, String> {
    jet_hinter.parse_simplicity(source)
}

pub fn parse_export<J: Jet>(source: &str) -> Result<Arc<CommitNode>, String> {
    let forest = Forest::parse::<J>(source).map_err(|errors| errors.to_string())?;
    let root = forest
        .roots()
        .get(EXPORT_ROOT)
        .ok_or_else(|| format!("no root entry `{EXPORT_ROOT}`"))?;
    Ok(root.to_commit_node())
}
