//! Where the raw Simplicity files of `extern fn` declarations come from, and their keys.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::driver::SourceMap;
use crate::source::CanonPath;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawSource {
    Text(Arc<str>),
    File(PathBuf),
}

/// Used for flattening raw Simplicity sources.
/// Supports both [`RawSource::Text`] and [`RawSource::File`] export approaches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawSources {
    files: BTreeMap<Arc<str>, RawSource>,
}

impl RawSources {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_disk() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<Arc<str>>, text: impl Into<Arc<str>>) {
        self.insert_source(path, RawSource::Text(text.into()));
    }

    pub fn insert_file(&mut self, path: impl Into<Arc<str>>, file: impl Into<PathBuf>) {
        self.insert_source(path, RawSource::File(file.into()));
    }

    pub fn insert_source(&mut self, path: impl Into<Arc<str>>, source: RawSource) {
        self.files.insert(path.into(), source);
    }

    pub fn get(&self, path: &str) -> Option<&RawSource> {
        self.files.get(path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Arc<str>, &RawSource)> {
        self.files.iter()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl<P: Into<Arc<str>>, T: Into<Arc<str>>> FromIterator<(P, T)> for RawSources {
    fn from_iter<I: IntoIterator<Item = (P, T)>>(iter: I) -> Self {
        let mut sources = Self::new();
        for (path, text) in iter {
            sources.insert(path, text);
        }
        sources
    }
}

/// A raw file an `extern fn` of a flattened program refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDependency {
    pub path: Arc<str>,
    pub file: PathBuf,
}

/// An `extern fn` entry a program imported. Contains [`crate::raw_simplicity::EXPORT_ROOT`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawImport {
    pub path: Arc<str>,
    pub entry: Arc<str>,
    pub cmr: simplicity::Cmr,
}

/// Locates raw Simplicity source code dependency.
pub fn locate(
    path: &str,
    file_id: usize,
    sources: Option<&SourceMap>,
) -> Option<(Arc<str>, Option<PathBuf>)> {
    let declaring_dir = sources
        .and_then(|s| s.path(file_id))
        .and_then(|file| file.as_path().parent());

    let file = match declaring_dir {
        Some(dir) => dir.join(path),
        None if Path::new(path).is_absolute() => PathBuf::from(path),
        None => return Some((Arc::from(path), None)),
    };

    // TODO: handle error
    let file = CanonPath::canonicalize(&file).map_or(file, |file| file.as_path().to_path_buf());

    let entry_dir = sources
        .and_then(|s| s.path(crate::driver::MAIN_MODULE))
        .and_then(|file| file.as_path().parent());

    let key = match entry_dir {
        Some(dir) if Path::new(path).is_relative() => pathdiff::diff_paths(&file, dir)?,
        _ => file.clone(),
    };

    let key = key.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");

    Some((Arc::from(key), Some(file)))
}
