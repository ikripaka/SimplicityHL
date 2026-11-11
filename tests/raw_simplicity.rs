use simplicityhl::value::ValueConstructible;
use simplicityhl::WitnessNameToValueMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use simplicityhl::ast::ElementsJetHinter;
use simplicityhl::resolution::DependencyMapBuilder;
use simplicityhl::source::{CanonPath, CanonSourceFile};
use simplicityhl::{
    Arguments, CompiledProgram, TemplateProgramWitness, UnstableFeatures, Value, WitnessValues,
};

const DECL: &str = "#[simplicity(path = \"equal32.simpl\", entry = \"equal32\")] extern fn raw_equal32(a: u32, b: u32) -> bool;";
const RAW: &str = "equal32 := jet_eq_32 : (2^32 * 2^32) -> 2\n";

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
        CanonPath::canonicalize(&path).unwrap(),
        Arc::from(std::fs::read_to_string(path).unwrap()),
    );
    let deps = DependencyMapBuilder::new()
        .build(CanonPath::canonicalize(root).unwrap())
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
fn invalid_declarations_fail_even_when_unused() {
    for (name, decl, raw, expected) in [
        (
            "signature",
            DECL.replace("-> bool", "-> u32"),
            RAW,
            "declared",
        ),
        (
            "entry",
            DECL.replace("entry = \"equal32\"", "entry = \"missing\""),
            RAW,
            "no root entry",
        ),
        (
            "file",
            DECL.replace("equal32.simpl", "missing.simpl"),
            RAW,
            "cannot read",
        ),
        ("parse", DECL.into(), "equal32 := comp", "equal32.simpl"),
        (
            "witness",
            DECL.into(),
            "equal32 := witness : (2^32 * 2^32) -> 2",
            "witness or disconnect",
        ),
        (
            "hole",
            DECL.into(),
            "equal32 := ?hole : (2^32 * 2^32) -> 2",
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
        ("main.simf", "#[simplicity(path = \"id.simpl\", entry = \"id\")] extern fn id(a: u32) -> u32; fn main() { assert!(jet::eq_32(id(42), 42)); }"),
        ("id.simpl", "id := iden : 2^32 -> 2^32"),
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
    let flat = simplicityhl::TemplateAst::flatten(source, &deps, &UnstableFeatures::all()).unwrap();
    CompiledProgram::new_with_unstable(
        flat,
        &UnstableFeatures::all(),
        Arguments::default(),
        false,
        Box::new(ElementsJetHinter::new()),
    )
    .unwrap();
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
    assert!(error.contains("named source file"), "{error}");
}

#[test]
fn raw_main_and_disconnect_are_rejected() {
    for (name, source, raw, expected) in [
        ("raw-main", "#[simplicity(path = \"equal32.simpl\", entry = \"equal32\")] extern fn main();", "equal32 := unit", "main must have"),
        ("disconnect", "#[simplicity(path = \"equal32.simpl\", entry = \"equal32\")] extern fn f(); fn main() {}", "equal32 := disconnect (pair unit unit) ?hole", "witness or disconnect"),
    ] {
        let root = project(name, &[("main.simf", source), ("equal32.simpl", raw)]);
        let error = compile(&root, true).unwrap_err();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn parsing_uses_the_selected_jet_set() {
    use simplicityhl::ast::{CoreJetHinter, JetHinter};
    assert!(CoreJetHinter::new().parse_simplicity(RAW).is_ok());
    let elements_only = "index := jet_current_index";
    assert!(ElementsJetHinter::new()
        .parse_simplicity(elements_only)
        .is_ok());
    assert!(CoreJetHinter::new()
        .parse_simplicity(elements_only)
        .is_err());
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
    let error = compile(&root, true).unwrap_err();
    assert!(error.contains("MissingType"), "{error}");
    assert!(!error.contains("undefined function"), "{error}");
}
