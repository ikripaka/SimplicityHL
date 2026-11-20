use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use simplicityhl::ast::{CoreJetHinter, ElementsJetHinter};
use simplicityhl::num::U256;
use simplicityhl::parse::parse_simplicity;
use simplicityhl::resolution::DependencyMapBuilder;
use simplicityhl::simplicity::{self, types, BitMachine, RedeemNode};
use simplicityhl::source::{CanonPath, CanonSourceFile};
use simplicityhl::types::TypeConstructible;
use simplicityhl::value::ValueConstructible;
use simplicityhl::{
    Arguments, CompiledProgram, ResolvedType, TemplateProgramWitness, UnstableFeature,
    UnstableFeatures, Value, WitnessNameToValueMap, WitnessValues,
};

// An imported `export` can return a value through the public Bit Machine API.
fn raw_function(text: &str) -> Result<Arc<RedeemNode>, Box<dyn Error>> {
    let root = parse_simplicity(text, &CoreJetHinter)?;

    // Preserve finalized arrows; these fragments have no witnesses to populate.
    Ok(root.finalize(&mut simplicity::node::SimpleFinalizer::new(
        std::iter::empty(),
    ))?)
}

// TODO: rewrite
#[test]
fn typed_raw_results_and_hl_calls() -> Result<(), Box<dyn Error>> {
    let identity = raw_function(include_str!("../examples/raw_simplicity/identity.simpl"))?;
    let choose = raw_function(include_str!("../examples/raw_simplicity/choose.simpl"))?;

    // Named sources resolve .simpl paths relative to the declaring .simf file.
    let root = CanonPath::canonicalize(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/raw_simplicity"),
    )?;
    let path = CanonPath::canonicalize(&root.as_path().join("typed_results.simf"))?;

    let source = CanonSourceFile::new(
        path.clone(),
        Arc::from(std::fs::read_to_string(path.as_path())?),
    );

    let dependencies = DependencyMapBuilder::new().build(root)?;
    let compiled = CompiledProgram::new_with_dep(
        source,
        &dependencies,
        &UnstableFeatures::new([UnstableFeature::Simplicity]),
        Arguments::default(),
        false,
        Box::new(ElementsJetHinter::new()),
    )?;

    // Check zero, ordinary input, each 64-bit carry boundary, selector boundary,
    // distinct high/low bits, and full overflow. Byte arrays are big-endian.
    let mut inputs = vec![[0; 32]];
    let mut ordinary = [0; 32];
    ordinary[31] = 42;
    inputs.push(ordinary);
    for low_bytes in [8, 16, 24, 32] {
        let mut input = [0; 32];
        input[32 - low_bytes..].fill(0xff);
        inputs.push(input);
    }

    let mut below_selector = [0xff; 32];
    below_selector[0] = 0x7f;
    inputs.push(below_selector);
    let mut selector = [0; 32];
    selector[0] = 0x80;
    inputs.push(selector);
    let mut mixed = [0xab; 32];
    mixed[28..].copy_from_slice(&0x12345678_u32.to_be_bytes());
    inputs.push(mixed);

    let env = simplicityhl::dummy_env::dummy();
    for input in &inputs {
        // Independent byte-wise wrapping increment supplies the expected result.
        let mut incremented = *input;
        for byte in incremented.iter_mut().rev() {
            let (next, carry) = byte.overflowing_add(1);
            *byte = next;
            if !carry {
                break;
            }
        }
        let low = u32::from_be_bytes(input[28..].try_into()?);
        let x = Value::u256(U256::from_byte_array(*input));
        let expected_increment = Value::u256(U256::from_byte_array(incremented));
        let (expected_raw_choice, expected_choice) = if input[0] & 0x80 == 0 {
            (
                simplicity::Value::left(
                    simplicity::Value::u256(*input),
                    types::Final::two_two_n(5)?,
                ),
                Value::left(x.clone(), ResolvedType::u32()),
            )
        } else {
            (
                simplicity::Value::right(types::Final::two_two_n(8)?, simplicity::Value::u32(low)),
                Value::right(ResolvedType::u256(), Value::u32(low)),
            )
        };

        for (function, expected) in [
            (&identity, simplicity::Value::u256(incremented)),
            (&choose, expected_raw_choice),
        ] {
            let mut machine = BitMachine::for_program(function)?;
            machine.input(&simplicity::Value::u256(*input))?;
            let result = machine.exec(function, &simplicity::jet::CoreEnv::new())?;
            assert_eq!(result, expected);
        }

        // HL consumes both return values, comparing them with expected witnesses.
        let witnesses = WitnessValues::from_map(HashMap::from([
            (TemplateProgramWitness::witness_from_str("X"), x),
            (
                TemplateProgramWitness::witness_from_str("EXPECTED_INCREMENT"),
                expected_increment,
            ),
            (
                TemplateProgramWitness::witness_from_str("EXPECTED_CHOICE"),
                expected_choice,
            ),
        ]));
        let satisfied = compiled.satisfy(witnesses)?;
        let mut machine = BitMachine::for_program(satisfied.redeem())?;
        assert_eq!(
            machine.exec(satisfied.redeem(), &env)?,
            simplicity::Value::unit()
        );
    }
    Ok(())
}
