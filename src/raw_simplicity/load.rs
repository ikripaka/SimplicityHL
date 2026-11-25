use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::ast::JetHinter;
use crate::error::{Diagnostic, DiagnosticManager, Error};
use crate::parse;

use super::{RawCode, RawFile, RawFiles, RawSources};

/// Injects into every `extern fn` of `program` its [`RawCode`].
///
/// The text of a raw file comes from `raw_files`, which the driver loaded
/// ([`crate::driver::DependencyGraph::build_program`]) and keyed by the paths it rewrote the
/// declarations to. A program that was not built by the driver has no loaded files; its
/// texts are given in `provided`, by declared path.
pub fn load_raw_code(
    program: &mut parse::Program,
    raw_files: &RawFiles,
    provided: &RawSources,
    jet_hinter: Box<dyn JetHinter>,
    diagnostics: &mut DiagnosticManager,
) -> Option<Vec<Arc<str>>> {
    let errors_before = diagnostics.error_count();

    // The keys of the imported files, once each, in declaration order.
    let mut imports = Vec::new();
    let mut codes = BTreeMap::<Arc<str>, RawCode>::new();

    program.for_each_extern_function_mut(|function| {
        let key: Arc<str> = Arc::from(function.path());
        let code = match codes.get(&key) {
            Some(code) => Ok(code.clone()),
            None => file_of(&key, raw_files, provided).and_then(|RawFile { origin, text }| {
                let code = RawCode::parse(&text, origin, &*jet_hinter)?;
                imports.push(Arc::clone(&key));
                codes.insert(key, code.clone());
                Ok(code)
            }),
        };

        match code {
            // Inserts a raw code into ExternFunction for later processing
            Ok(code) => function.set_code(code),
            Err(error) => diagnostics.push(Diagnostic::new(error, *function.span())),
        }
    });

    (diagnostics.error_count() == errors_before).then_some(imports)
}

/// The raw file with the given `key`.
fn file_of(key: &Arc<str>, raw_files: &RawFiles, provided: &RawSources) -> Result<RawFile, Error> {
    if let Some(raw) = raw_files.get(key) {
        return Ok(raw.clone());
    }
    if let Some(text) = provided.get(key) {
        return Ok(RawFile {
            origin: Arc::from(Path::new(key.as_ref())),
            text: Arc::clone(text),
        });
    }

    // Without the driver there is no source file, so only an absolute path can be read.
    let path = Path::new(key.as_ref());
    if path.is_relative() {
        return Err(Error::Simplicity {
            msg: format!(
                "the relative external path `{key}` of a source without a file needs its text: give it with RawSources"
            ),
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::Simplicity {
        msg: format!("cannot read {}: {e}", path.display()),
    })?;
    Ok(RawFile {
        origin: Arc::from(path),
        text: Arc::from(text),
    })
}
