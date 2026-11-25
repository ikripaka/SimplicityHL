//! The raw Simplicity files of `extern fn` declarations, and their keys.
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// The texts of raw Simplicity files, by key: the path of the file an `extern fn` names,
/// relative to the project root (the package root of the entry file). Without a project, the
/// key is the declared path. A given text replaces the file on disk.
pub type RawSources = BTreeMap<Arc<str>, Arc<str>>;

/// The text of a raw Simplicity file.
#[derive(Clone, Debug)]
pub(crate) struct RawFile {
    /// Where the text came from, for error messages: the file on disk, or the key of a
    /// given text.
    pub origin: Arc<Path>,
    pub text: Arc<str>,
}

/// The raw files the driver loaded, by key.
pub(crate) type RawFiles = BTreeMap<Arc<str>, RawFile>;
