use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use simplicityhl::ast::ElementsJetHinter;
use simplicityhl::error::{DiagnosticManager, Error, Location, Severity};
use simplicityhl::resolution::DependencyMapBuilder;
use simplicityhl::source::{CanonPath, CanonSourceFile};
use simplicityhl::value::ValueConstructible;
use simplicityhl::WitnessNameToValueMap;
use simplicityhl::{
    Arguments, CompiledProgram, RawOrigin, RawSources, TemplateProgramWitness, UnstableFeatures,
    Value, WitnessValues,
};

const DECL: &str = "#[path = \"equal32.simpl\"] extern fn raw_equal32(a: u32, b: u32) -> bool;";
const RAW: &str = "export := jet_eq_32 : (2^32 * 2^32) -> 2\n";

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("raw-simplicity-{name}"));

    for (name, content) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    root
}

fn compile(root: &Path, enabled: bool) -> Result<CompiledProgram, String> {
    let path = root.join("main.simf");
    let source = CanonSourceFile::new(
        CanonPath::canonicalize(&path)?,
        Arc::from(std::fs::read_to_string(path).unwrap()),
    );

    let deps = DependencyMapBuilder::new()
        .build(CanonPath::canonicalize(root)?)
        .unwrap();

    CompiledProgram::new_with_dep(
        source,
        &deps,
        &if enabled {
            UnstableFeatures::all()
        } else {
            UnstableFeatures::none()
        },
        Arguments::default(),
        false,
        Box::new(ElementsJetHinter::new()),
    )
}

fn analyze(root: &Path) -> Result<simplicityhl::TemplateAst, DiagnosticManager> {
    let path = root.join("main.simf");
    let source = CanonSourceFile::new(
        CanonPath::canonicalize(&path).unwrap(),
        Arc::from(std::fs::read_to_string(path).unwrap()),
    );

    let deps = DependencyMapBuilder::new()
        .build(CanonPath::canonicalize(root).unwrap())
        .unwrap();

    simplicityhl::TemplateAst::new_with_dep(
        source,
        &deps,
        &UnstableFeatures::all(),
        Box::new(ElementsJetHinter::new()),
    )
}

#[test]
fn structured_signature_mismatches() {
    use simplicityhl::types::{StructuralType, TypeConstructible};
    use simplicityhl::ResolvedType;

    for (name, decl, raw, mismatch, input, output) in [
        (
            "input",
            DECL.replace("a: u32", "a: u16"),
            RAW,
            "input",
            StructuralType::tuple([StructuralType::u32(), StructuralType::u32()]),
            StructuralType::boolean(),
        ),
        (
            "output",
            DECL.replace("-> bool", "-> u32"),
            RAW,
            "output",
            StructuralType::tuple([StructuralType::u32(), StructuralType::u32()]),
            StructuralType::boolean(),
        ),
        (
            "both",
            DECL.replace("a: u32", "a: u16")
                .replace("-> bool", "-> u32"),
            RAW,
            "input and output",
            StructuralType::tuple([StructuralType::u32(), StructuralType::u32()]),
            StructuralType::boolean(),
        ),
    ] {
        let source = format!("{decl}\nfn main() {{}}");
        let root = project(
            &format!("structured-{name}"),
            &[("main.simf", &source), ("equal32.simpl", raw)],
        );

        let errors = analyze(&root).unwrap_err();
        assert_eq!(errors.error_count(), 1);

        let diag = &errors.diagnostics()[0];
        assert_eq!(*diag.severity(), Severity::Error);

        let Location::Code(span) = diag.location() else {
            panic!("expected declaration span")
        };
        assert_eq!(
            errors
                .sources()
                .unwrap()
                .path(span.file_id)
                .unwrap()
                .as_path(),
            root.join("main.simf").canonicalize().unwrap()
        );

        let Error::SimplicityTypeMismatch {
            origin,
            entry,
            expected_input,
            expected_output,
            found_input,
            found_output,
        } = diag.error()
        else {
            panic!("unexpected error: {diag:?}")
        };

        assert_eq!(
            origin,
            &RawOrigin::Extern(root.join("equal32.simpl").canonicalize().unwrap().into())
        );
        assert_eq!(entry, "export");

        let expected_first = if name == "output" {
            ResolvedType::u32()
        } else {
            ResolvedType::u16()
        };

        let expected_ret = if name == "input" {
            ResolvedType::boolean()
        } else {
            ResolvedType::u32()
        };

        assert_eq!(
            *expected_input,
            Box::new(ResolvedType::tuple([expected_first, ResolvedType::u32()]))
        );
        assert_eq!(**expected_output, expected_ret);
        assert_eq!(**found_input, input);
        assert_eq!(**found_output, output);
        assert_eq!(
            StructuralType::from(&*expected_input.clone()) != **found_input,
            name != "output"
        );
        assert_eq!(
            StructuralType::from(&*expected_output.clone()) != **found_output,
            name != "input"
        );

        let rendered = errors.render_to_string();

        for part in [
            "equal32.simpl",
            "entry `export`",
            mismatch,
            "declared HL",
            "expected structural",
            "found",
            "annotating",
        ] {
            assert!(rendered.contains(part), "missing {part}: {rendered}");
        }
    }
}

#[test]
fn broken_enum_payload_in_each_signature_position_is_not_lowered() {
    for (name, signature) in [("parameter", "bad(a: E)"), ("return", "bad() -> E")] {
        let source = format!("enum E {{ Bad(Missing), Good }}\n#[path = \"equal32.simpl\"] extern fn {signature};\nfn main() {{}}");
        let root = project(
            &format!("enum-{name}-recovery"),
            &[("main.simf", &source), ("equal32.simpl", RAW)],
        );

        let errors = analyze(&root).unwrap_err();
        let rendered = errors.render_to_string();

        assert!(rendered.contains("Missing"), "{rendered}");
        assert!(!errors
            .diagnostics()
            .iter()
            .any(|d| matches!(d.error(), Error::SimplicityTypeMismatch { .. })));
    }
}

#[test]
fn imported_mismatch_points_to_the_declaration() {
    let decl = DECL
        .replace("extern fn", "pub extern fn")
        .replace("-> bool", "-> u32");
    let root = project(
        "imported-mismatch",
        &[
            (
                "main.simf",
                "use crate::nested::lib::raw_equal32; fn main() { let _: u32 = raw_equal32(1, 1); }",
            ),
            ("nested/lib.simf", &decl),
            ("nested/equal32.simpl", RAW),
        ],
    );

    let errors = analyze(&root).unwrap_err();
    assert_eq!(errors.error_count(), 1, "{errors}");

    let diag = errors
        .diagnostics()
        .iter()
        .find(|d| matches!(d.error(), Error::SimplicityTypeMismatch { .. }))
        .unwrap();

    let Location::Code(span) = diag.location() else {
        panic!("expected declaration span")
    };
    assert_eq!(
        errors
            .sources()
            .unwrap()
            .path(span.file_id)
            .unwrap()
            .as_path(),
        root.join("nested/lib.simf").canonicalize().unwrap()
    );

    let rendered = errors.render_to_string();
    assert!(rendered.contains("nested/lib.simf"), "{rendered}");
    assert!(rendered.contains("nested/equal32.simpl"), "{rendered}");
}

#[test]
fn malformed_extern_signature_stops_analysis() {
    let source = "enum E { Bad(Missing), Good }\n\
        #[path = \"equal32.simpl\"] extern fn bad(a: E) -> E;\n\
        #[path = \"equal32.simpl\"] extern fn wrong(a: u16, b: u32) -> bool;\n\
        fn main() { let e: E = E::Good; let x: E = bad(e); assert!(wrong(1, 1)); }";
    let root = project(
        "enum-recovery",
        &[("main.simf", source), ("equal32.simpl", RAW)],
    );

    let errors = analyze(&root).unwrap_err();
    assert!(!errors
        .diagnostics()
        .iter()
        .any(|d| matches!(d.error(), Error::SimplicityTypeMismatch { .. })));

    let rendered = errors.render_to_string();
    assert!(rendered.contains("Missing"), "{rendered}");
    assert!(
        !rendered.contains("not defined as a function"),
        "{rendered}"
    );
}

#[test]
fn raw_load_error_is_reported_before_analysis() {
    let source = "#[path = \"absent.simpl\"] extern fn bad(a: u32) -> bool;\n\
        fn main() { let x: Missing = 0; }";
    let root = project("load-error-first", &[("main.simf", source)]);
    let errors = analyze(&root).unwrap_err();

    let rendered = errors.render_to_string();
    assert!(rendered.contains("cannot read"), "{rendered}");
    // Without the raw code nothing can be checked, so the program is not analyzed.
    assert!(!rendered.contains("Missing"), "{rendered}");
}

#[test]
fn raw_structural_shape_is_checked_and_equivalence_is_allowed() {
    for (name, decl, raw, valid) in [
        (
            "equivalent",
            "extern fn f(a: u16) -> u16;",
            "export := iden : (2^8 * 2^8) -> (2^8 * 2^8)",
            true,
        ),
        (
            "shape",
            "extern fn f(a: u2) -> u2;",
            "export := iden : (1 + (1 + 1)) -> (1 + (1 + 1))",
            false,
        ),
        (
            "inferred",
            "extern fn f() -> bool;",
            "export := injl unit",
            true,
        ),
    ] {
        let source = format!("#[path = \"f.simpl\"] {decl}\nfn main() {{}}");
        let root = project(
            &format!("structure-{name}"),
            &[("main.simf", &source), ("f.simpl", raw)],
        );

        let result = analyze(&root);
        if valid {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            assert!(result
                .unwrap_err()
                .diagnostics()
                .iter()
                .any(|d| matches!(d.error(), Error::SimplicityTypeMismatch { .. })));
        }
    }
}

#[test]
fn extern_load_failure_stops_at_the_first_declaration() {
    let first = DECL
        .replace("raw_equal32", "first")
        .replace("a: u32", "a: u16");
    let second = DECL
        .replace("raw_equal32", "second")
        .replace("-> bool", "-> u32");
    let source = format!(
        "{first}\n{second}\nfn main() {{ assert!(first(1, 1)); let x: u32 = second(1, 1); }}"
    );

    let root = project(
        "multiple-bad",
        &[("main.simf", &source), ("equal32.simpl", RAW)],
    );
    let errors = analyze(&root).unwrap_err();

    let [diagnostic] = errors.diagnostics() else {
        panic!("expected one diagnostic: {errors}");
    };
    assert!(matches!(
        diagnostic.error(),
        Error::SimplicityTypeMismatch { .. }
    ));

    let Location::Code(span) = diagnostic.location() else {
        panic!("expected declaration span: {errors}");
    };
    assert_eq!(span.to_slice(&source), Some(first.as_str()));
}

#[test]
fn external_calls_use_hl_type_rules() {
    for (name, prefix, call, expected) in [
        ("count", "", "raw_equal32(1)", "count"),
        ("argument", "", "raw_equal32(true, 1)", "type"),
        (
            "result",
            "",
            "{ let x: u32 = raw_equal32(1, 1); true }",
            "type",
        ),
        ("overflow", "", "raw_equal32(4294967296, 1)", "integer"),
    ] {
        let source = format!("{prefix}\n{DECL}\nfn main() {{ assert!({call}); }}");
        let root = project(
            &format!("hl-call-{name}"),
            &[("main.simf", &source), ("equal32.simpl", RAW)],
        );

        let errors = analyze(&root).unwrap_err();
        assert!(
            errors.diagnostics().iter().any(|d| match expected {
                "count" => matches!(d.error(), Error::InvalidNumberOfArguments { .. }),
                "type" => matches!(d.error(), Error::ExpressionTypeMismatch { .. }),
                "integer" => matches!(
                    d.error(),
                    Error::ParseInt { .. } | Error::ParseCrateInt { .. }
                ),
                _ => false,
            }),
            "{name}: {errors}"
        );
    }
}

#[test]
fn external_calls_reject_distinct_enums() {
    let source = "enum E { A, B } enum F { A, B }\n\
        #[path = \"f.simpl\"] extern fn f(a: E) -> E;\n\
        fn main() { let x: F = F::A; let y: E = f(x); }";
    let root = project(
        "hl-call-enums",
        &[
            ("main.simf", source),
            ("f.simpl", "export := iden : 2 -> 2"),
        ],
    );

    let errors = analyze(&root).unwrap_err();
    assert!(
        errors
            .diagnostics()
            .iter()
            .any(|d| matches!(d.error(), Error::ExpressionTypeMismatch { .. })),
        "{errors}"
    );
}

#[test]
fn raw_loading_errors_keep_their_category() {
    for (name, raw) in [
        ("missing-file", None),
        ("missing-export", Some("other := unit")),
    ] {
        let source = format!("{DECL}\nfn main() {{}}");
        let files = if let Some(raw) = raw {
            vec![("main.simf", source.as_str()), ("equal32.simpl", raw)]
        } else {
            vec![("main.simf", source.as_str())]
        };

        let root = project(&format!("category-{name}"), &files);

        let errors = analyze(&root).unwrap_err();
        assert!(errors
            .diagnostics()
            .iter()
            .any(|d| matches!(d.error(), Error::Simplicity { .. })));
        assert!(!errors
            .diagnostics()
            .iter()
            .any(|d| matches!(d.error(), Error::SimplicityTypeMismatch { .. })));
    }
}

#[test]
fn raw_function_executes_with_hl_witnesses_and_multiple_calls() {
    let source = format!("{DECL}\nfn main() {{ assert!(raw_equal32(witness::A, witness::B)); assert!(raw_equal32(7, 7)); }}");
    let root = project("execute", &[("main.simf", &source), ("equal32.simpl", RAW)]);
    let program = compile(&root, true).unwrap();
    let env = simplicityhl::dummy_env::dummy();

    for (a, b, succeeds) in [
        (42, 42, true),
        (42, 43, false),
        (0, 0, true),
        (u32::MAX, u32::MAX, true),
    ] {
        let witnesses = WitnessValues::from_map(HashMap::from([
            (TemplateProgramWitness::witness_from_str("A"), Value::u32(a)),
            (TemplateProgramWitness::witness_from_str("B"), Value::u32(b)),
        ]));
        let satisfied = program.satisfy(witnesses).unwrap();

        let mut machine = simplicity::BitMachine::for_program(satisfied.redeem()).unwrap();
        assert_eq!(machine.exec(satisfied.redeem(), &env).is_ok(), succeeds);
    }
}

#[test]
fn extern_functions_work_as_fold_and_loop_callbacks() {
    let source = r#"
        #[path = "add.simpl"] extern fn add(a: u32, b: u32) -> u32;
        #[path = "step.simpl"] extern fn step(acc: u32, ctx: (), counter: u1) -> Either<u32, u32>;

        fn main() {
            let list_sum: u32 = fold::<add, 4>(list![1, 2, 3], 10);
            let array_sum: u32 = array_fold::<add, 3>([4, 5, 6], 10);
            let loop_result: Either<u32, u32> = for_while::<step>(7, ());
            assert!(jet::eq_32(list_sum, 16));
            assert!(jet::eq_32(array_sum, 25));
            assert!(jet::eq_32(unwrap_left::<u32>(loop_result), 7));
        }
    "#;
    let raw_sources = RawSources::from_iter([
        ("add.simpl", "export := comp jet_add_32 (drop iden)"),
        (
            "step.simpl",
            "acc := iden : 2^32 -> 2^32\n\
             project_acc := take acc : (2^32 * (1 * 2)) -> 2^32\n\
             export := injl project_acc : _ -> (2^32 + 2^32)",
        ),
    ]);
    let program = CompiledProgram::new_with_raw_sources(
        source,
        &UnstableFeatures::all(),
        &raw_sources,
        Arguments::default(),
        false,
        Box::new(ElementsJetHinter),
    )
    .unwrap();

    let satisfied = program.satisfy(WitnessValues::default()).unwrap();
    let mut machine = simplicity::BitMachine::for_program(satisfied.redeem()).unwrap();
    assert_eq!(
        machine
            .exec(satisfied.redeem(), &simplicityhl::dummy_env::dummy())
            .unwrap(),
        simplicity::Value::unit()
    );
}

#[test]
fn invalid_declarations_fail_even_when_unused() {
    for (name, decl, raw, expected) in [
        (
            "signature",
            DECL.replace("-> bool", "-> u32"),
            RAW,
            "declared",
        ),
        (
            "missing-export",
            DECL.into(),
            "main := unit",
            "no root entry `export`",
        ),
        (
            "file",
            DECL.replace("equal32.simpl", "missing.simpl"),
            RAW,
            "cannot read",
        ),
        ("parse", DECL.into(), "export := comp", "equal32.simpl"),
        (
            "witness",
            DECL.into(),
            "export := witness : (2^32 * 2^32) -> 2",
            "witness or disconnect",
        ),
        (
            "hole",
            DECL.into(),
            "export := ?hole : (2^32 * 2^32) -> 2",
            "equal32.simpl",
        ),
    ] {
        let source = format!("{decl}\nfn main() {{}}");
        let root = project(name, &[("main.simf", &source), ("equal32.simpl", raw)]);

        let error = compile(&root, true).unwrap_err();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn external_syntax_requires_feature() {
    let source = format!("{DECL}\nfn main() {{}}");
    let root = project("gate", &[("main.simf", &source)]);

    let error = compile(&root, false).unwrap_err();
    assert!(error.contains("simplicity"), "{error}");
    assert!(
        !error.contains("cannot read"),
        "feature checks must precede IO: {error}"
    );
}

#[test]
fn imported_module_resolves_raw_path_relative_to_declaring_file() {
    let lib = DECL.replace("extern fn", "pub extern fn");
    let root = project(
        "module",
        &[
            (
                "main.simf",
                "use crate::nested::lib::raw_equal32; fn main() { assert!(raw_equal32(1, 1)); }",
            ),
            ("nested/lib.simf", &lib),
            ("nested/equal32.simpl", RAW),
        ],
    );
    compile(&root, true).unwrap();
}

#[test]
fn typed_identity_keeps_its_annotations() {
    let root = project("identity", &[
        ("main.simf", "#[path = \"id.simpl\"] extern fn id(a: u32) -> u32; fn main() { assert!(jet::eq_32(id(42), 42)); }"),
        ("id.simpl", "export := iden : 2^32 -> 2^32"),
    ]);
    let program = compile(&root, true)
        .unwrap()
        .satisfy(WitnessValues::default())
        .unwrap();

    let mut machine = simplicity::BitMachine::for_program(program.redeem()).unwrap();
    machine
        .exec(program.redeem(), &simplicityhl::dummy_env::dummy())
        .unwrap();
}

#[test]
fn raw_and_hl_equality_size_and_cost() {
    let main = "fn main() { assert!(raw_equal32(witness::A, witness::B)); }";
    let raw_source = format!("{DECL}\n{main}");
    let hl_source =
        format!("fn raw_equal32(a: u32, b: u32) -> bool {{ jet::eq_32(a, b) }}\n{main}");
    let raw_root = project(
        "measure-raw",
        &[("main.simf", &raw_source), ("equal32.simpl", RAW)],
    );
    let hl_root = project("measure-hl", &[("main.simf", &hl_source)]);

    let witnesses = WitnessValues::from_map(HashMap::from([
        (
            TemplateProgramWitness::witness_from_str("A"),
            Value::u32(42),
        ),
        (
            TemplateProgramWitness::witness_from_str("B"),
            Value::u32(42),
        ),
    ]));

    let env = simplicityhl::dummy_env::dummy();

    let mut measurements = Vec::new();

    for (label, root) in [("raw", raw_root), ("HL", hl_root)] {
        let program = compile(&root, true)
            .unwrap()
            .satisfy(witnesses.clone())
            .unwrap();
        let pruned = program.redeem().prune(&env).unwrap();

        let mut machine = simplicity::BitMachine::for_program(&pruned).unwrap();
        machine.exec(&pruned, &env).unwrap();

        let (bytes, _) = pruned.to_vec_with_witness();
        let cost = pruned.bounds().cost;
        println!(
            "{label}: {} program bytes, {cost} cost (debug disabled, pruned)",
            bytes.len()
        );
        measurements.push((bytes.len(), cost));
    }
    assert!(measurements[0].0 <= measurements[1].0);
    assert!(measurements[0].1 <= measurements[1].1);
}

#[test]
fn flatten_preserves_raw_paths_from_imported_modules() {
    let lib = DECL.replace("extern fn", "pub extern fn");
    let root = project(
        "flatten-input",
        &[
            (
                "main.simf",
                "use crate::nested::lib::raw_equal32; fn main() { assert!(raw_equal32(1, 1)); }",
            ),
            ("nested/lib.simf", &lib),
            ("nested/equal32.simpl", RAW),
        ],
    );

    let path = root.join("main.simf");
    let source = CanonSourceFile::new(
        CanonPath::canonicalize(&path).unwrap(),
        Arc::from(std::fs::read_to_string(path).unwrap()),
    );

    let deps = DependencyMapBuilder::new()
        .build(CanonPath::canonicalize(&root).unwrap())
        .unwrap();
    let (flat, raw_deps) = simplicityhl::TemplateAst::flatten_with_raw_dependencies(
        source,
        &deps,
        &UnstableFeatures::all(),
    )
    .unwrap();

    // The path is relative to the entry file, not absolute.
    assert!(flat.contains(r#"path = "nested/equal32.simpl""#), "{flat}");
    assert!(!flat.contains(root.to_str().unwrap()), "{flat}");
    assert_eq!(raw_deps.len(), 1);
    assert_eq!(&*raw_deps[0].path, "nested/equal32.simpl");
    assert_eq!(
        raw_deps[0].file,
        root.join("nested/equal32.simpl").canonicalize().unwrap()
    );

    // The flattened source compiles from the files given by key, as does the original.
    let given: RawSources = raw_deps
        .iter()
        .map(|dep| {
            (
                dep.path.clone(),
                std::fs::read_to_string(&dep.file).unwrap(),
            )
        })
        .collect();

    let flattened = CompiledProgram::new_with_raw_sources(
        flat.as_str(),
        &UnstableFeatures::all(),
        &given,
        Arguments::default(),
        false,
        Box::new(ElementsJetHinter::new()),
    )
    .unwrap();

    let deps = DependencyMapBuilder::new()
        .build(CanonPath::canonicalize(&root).unwrap())
        .unwrap();
    let path = root.join("main.simf");
    let source = CanonSourceFile::new(
        CanonPath::canonicalize(&path).unwrap(),
        Arc::from(std::fs::read_to_string(path).unwrap()),
    );

    let original = simplicityhl::TemplateAst::new_with_dep_and_raw_sources(
        source,
        &deps,
        &UnstableFeatures::all(),
        Box::new(ElementsJetHinter::new()),
        &given,
    )
    .unwrap()
    .instantiate(Arguments::default(), false)
    .unwrap();
    assert_eq!(flattened.commit().cmr(), original.commit().cmr());
}

#[test]
fn raw_sources_are_provided_and_reported() {
    let source = format!("{DECL}\nfn main() {{ assert!(raw_equal32(7, 7)); }}");
    assert_eq!(
        simplicityhl::TemplateAst::declared_raw_paths(&source, &UnstableFeatures::all())
            .unwrap()
            .iter()
            .map(|p| p.as_ref())
            .collect::<Vec<_>>(),
        ["equal32.simpl"]
    );
    let given: RawSources = [("equal32.simpl", RAW)].into_iter().collect();

    let template = simplicityhl::TemplateAst::new_with_raw_sources(
        source.as_str(),
        &UnstableFeatures::all(),
        Box::new(ElementsJetHinter::new()),
        &given,
    )
    .unwrap();
    assert_eq!(template.raw_sources(), &given);

    let expected_cmr = simplicity::jet::Jet::cmr(&simplicity::jet::Elements::Eq32);
    let imports = template.raw_imports();
    assert_eq!(imports.len(), 1);
    assert_eq!(
        (&*imports[0].path, &*imports[0].entry),
        ("equal32.simpl", "export")
    );
    assert_eq!(imports[0].cmr, expected_cmr);

    // A file-based compilation reads from disk by default, and reports the file by key.
    let root = project(
        "raw-sources-reported",
        &[("main.simf", &source), ("equal32.simpl", RAW)],
    );
    assert_eq!(analyze(&root).unwrap().raw_sources(), &given);
}

#[test]
fn given_raw_sources_replace_the_file_system() {
    // On disk the raw file disagrees with the declaration; the given text agrees.
    let source = format!("{DECL}\nfn main() {{ assert!(raw_equal32(7, 7)); }}");
    let root = project(
        "raw-sources-given",
        &[
            ("main.simf", &source),
            ("equal32.simpl", "export := jet_eq_16 : (2^16 * 2^16) -> 2"),
        ],
    );

    let analyze_with = |given: &RawSources| {
        let path = root.join("main.simf");
        let source = CanonSourceFile::new(
            CanonPath::canonicalize(&path).unwrap(),
            Arc::from(std::fs::read_to_string(path).unwrap()),
        );
        let deps = DependencyMapBuilder::new()
            .build(CanonPath::canonicalize(&root).unwrap())
            .unwrap();
        simplicityhl::TemplateAst::new_with_dep_and_raw_sources(
            source,
            &deps,
            &UnstableFeatures::all(),
            Box::new(ElementsJetHinter::new()),
            given,
        )
    };
    assert!(analyze(&root).is_err(), "the file on disk mismatches");
    assert!(analyze_with(&[("equal32.simpl", RAW)].into_iter().collect()).is_ok());
    // A key that is not given is read from disk.
    assert!(
        analyze_with(&RawSources::new()).is_err(),
        "the file on disk is read"
    );

    // A key can name another file on disk instead.
    let elsewhere = project("raw-sources-given-elsewhere", &[("equal32.simpl", RAW)]);
    let mut given = RawSources::new();
    given.insert_file("equal32.simpl", elsewhere.join("equal32.simpl"));
    assert!(analyze_with(&given).is_ok());
}

#[test]
fn given_texts_and_files_on_disk_can_be_mixed() {
    // `equal32.simpl` is given as text (its file on disk is wrong); `identity.simpl` is read
    // from disk.
    let source = format!(
        "{DECL}\n\
         #[path = \"identity.simpl\"] extern fn identity(a: u32) -> u32;\n\
         fn main() {{ assert!(raw_equal32(identity(7), 7)); }}"
    );
    let identity = "export := iden : 2^32 -> 2^32\n";
    let root = project(
        "raw-sources-mixed",
        &[
            ("main.simf", &source),
            ("equal32.simpl", "export := jet_eq_16 : (2^16 * 2^16) -> 2"),
            ("identity.simpl", identity),
        ],
    );

    let path = root.join("main.simf");
    let source = CanonSourceFile::new(
        CanonPath::canonicalize(&path).unwrap(),
        Arc::from(std::fs::read_to_string(path).unwrap()),
    );

    let deps = DependencyMapBuilder::new()
        .build(CanonPath::canonicalize(&root).unwrap())
        .unwrap();
    let given: RawSources = [("equal32.simpl", RAW)].into_iter().collect();
    let template = simplicityhl::TemplateAst::new_with_dep_and_raw_sources(
        source,
        &deps,
        &UnstableFeatures::all(),
        Box::new(ElementsJetHinter::new()),
        &given,
    )
    .unwrap();

    // Both texts are reported, so the program can be compiled again from them alone.
    let loaded: RawSources = [("equal32.simpl", RAW), ("identity.simpl", identity)]
        .into_iter()
        .collect();
    assert_eq!(template.raw_sources(), &loaded);
    template.instantiate(Arguments::default(), false).unwrap();
}

#[test]
fn raw_keys_of_files_are_resolved_by_the_file_system() {
    // The declared path is appended to the directory of the declaring file and resolved there.
    let source = DECL.replace("equal32.simpl", "./nested/../equal32.simpl")
        + "\nfn main() { assert!(raw_equal32(7, 7)); }";
    let root = project(
        "resolved-keys",
        &[
            ("main.simf", &source),
            ("equal32.simpl", RAW),
            ("nested/keep", ""),
        ],
    );

    let template = analyze(&root).unwrap();
    assert_eq!(&*template.raw_imports()[0].path, "equal32.simpl");
}

#[test]
fn anonymous_sources_reject_external_file_resolution() {
    let source = format!("{DECL}\nfn main() {{}}");
    let error = simplicityhl::TemplateAst::new_with_unstable(
        source,
        &UnstableFeatures::all(),
        Box::new(ElementsJetHinter::new()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("RawSources"), "{error}");
}

#[test]
fn raw_main_and_disconnect_are_rejected() {
    for (name, source, raw, expected) in [
        (
            "raw-main-only",
            "#[path = \"equal32.simpl\"] extern fn main();",
            "export := unit",
            "The 'main' function must be defined in the entry point file",
        ),
        (
            "raw-main-with-entry-point",
            "#[path = \"equal32.simpl\"] extern fn main(); fn main() {}",
            "export := unit",
            "main must have",
        ),
        (
            "disconnect",
            "#[path = \"equal32.simpl\"] extern fn f(); fn main() {}",
            "export := disconnect (pair unit unit) ?hole",
            "witness or disconnect",
        ),
    ] {
        let root = project(name, &[("main.simf", source), ("equal32.simpl", raw)]);

        let error = compile(&root, true).unwrap_err();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn parsing_uses_the_selected_jet_set() {
    use simplicityhl::ast::CoreJetHinter;
    use simplicityhl::TemplateAst;

    let source = r#"
        #[path = "index.simpl"]
        extern fn index() -> u32;
        fn main() {}
    "#;
    let raw_sources = RawSources::from_iter([("index.simpl", "export := jet_current_index")]);
    let features = UnstableFeatures::all();

    TemplateAst::new_with_raw_sources(source, &features, Box::new(ElementsJetHinter), &raw_sources)
        .unwrap();

    let error =
        TemplateAst::new_with_raw_sources(source, &features, Box::new(CoreJetHinter), &raw_sources)
            .unwrap_err()
            .render_to_string();
    assert!(error.contains("current_index"), "{error}");
}

#[test]
fn invalid_signature_recovers_without_lowering_error_types() {
    let source = format!(
        "{}\nfn main() {{ assert!(raw_equal32(1, 1)); }}",
        DECL.replace("a: u32", "a: MissingType")
    );
    let root = project(
        "bad-signature",
        &[("main.simf", &source), ("equal32.simpl", RAW)],
    );

    let errors = analyze(&root).unwrap_err();
    assert!(
        errors.render_to_string().contains("MissingType"),
        "{errors}"
    );
    assert!(
        !errors
            .diagnostics()
            .iter()
            .any(|diag| matches!(diag.error(), Error::FunctionUndefined { .. })),
        "{errors}"
    );
}
