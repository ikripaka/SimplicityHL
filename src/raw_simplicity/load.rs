use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ast::JetHinter;
use crate::driver::SourceMap;
use crate::error::{Diagnostic, DiagnosticManager, Error};
use crate::parse;
use crate::source::CanonPath;

use super::sources::locate;
use super::{RawCode, RawImport, RawOrigin, RawSource, RawSources, EXPORT_ROOT};

#[derive(Debug)]
pub struct LoadedRaw {
    pub sources: RawSources,
    pub imports: Vec<RawImport>,
}

/// Injects into every `extern fn` of `program` its [`RawCode`] via obtaining the source file.
pub fn load_raw_code(
    program: &mut parse::Program,
    sources: Option<&SourceMap>,
    provided: &RawSources,
    jet_hinter: Box<dyn JetHinter>,
    diagnostics: &mut DiagnosticManager,
) -> Option<LoadedRaw> {
    let errors_before = diagnostics.error_count();

    let mut loaded = LoadedRaw {
        sources: RawSources::new(),
        imports: Vec::new(),
    };
    let mut codes = BTreeMap::<Arc<str>, RawCode>::new();

    program.for_each_extern_function_mut(|function| {
        let file_id = function.span().file_id;

        let code = locate(function.path(), file_id, sources)
            .ok_or_else(|| Error::Simplicity {
                msg: format!("cannot resolve the external path `{}`", function.path()),
            })
            .and_then(|(key, file)| {
                if let Some(code) = codes.get(&key) {
                    return Ok((key, code.clone()));
                }

                let (origin, text) = read(function.path(), &key, file, provided)?;

                let code = RawCode::parse(&text, origin, &*jet_hinter)?;

                loaded.sources.insert(Arc::clone(&key), text);
                codes.insert(Arc::clone(&key), code.clone());

                Ok((key, code))
            });

        match code {
            Ok((key, code)) => {
                loaded.imports.push(RawImport {
                    path: key,
                    entry: Arc::from(EXPORT_ROOT),
                    cmr: code.root().cmr(),
                });

                // Inserts a raw code into ExternFunction for later processing
                function.set_code(code);
            }
            Err(error) => diagnostics.push(Diagnostic::new(error, *function.span())),
        }
    });

    (diagnostics.error_count() == errors_before).then_some(loaded)
}

/// Transforms raw source into a [`RawOrigin`] for further processing.
fn read(
    path: &str,
    key: &Arc<str>,
    file: Option<PathBuf>,
    provided: &RawSources,
) -> Result<(RawOrigin, Arc<str>), Error> {
    let raw_error = |msg: String| Error::Simplicity { msg };

    let file = match provided.get(key) {
        Some(RawSource::Text(text)) => {
            return Ok((
                RawOrigin::Extern(Arc::from(Path::new(key.as_ref()))),
                Arc::clone(text),
            ))
        }
        Some(RawSource::File(file)) => {
            CanonPath::canonicalize(file).map_or_else(|_| file.clone(), |f| f.as_path().to_path_buf())
        }
        None => file.ok_or_else(|| {
            raw_error(format!(
                "the relative external path `{path}` of a source without a file needs its text: give it with RawSources"
            ))
        })?,
    };

    let text = std::fs::read_to_string(&file)
        .map_err(|e| raw_error(format!("cannot read {}: {e}", file.display())))?;

    Ok((RawOrigin::Extern(Arc::from(file)), Arc::from(text)))
}
