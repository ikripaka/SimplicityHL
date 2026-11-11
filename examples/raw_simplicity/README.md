# External raw Simplicity functions (experimental)

From the compiler repository root:

```sh
cargo run --bin simc -- -Z simplicity examples/raw_simplicity/main.simf
cargo test --test raw_simplicity -- --nocapture
```

The example compiles a SimplicityHL caller together with `equal32.simpl` into one
Simplicity program. `A` and `B` remain ordinary HL witnesses. The execution test
supplies equal and unequal values: equal values succeed, unequal values fail at
`assert!`. Compilation alone does not evaluate witnesses.

Declare a raw function using:

```rust
#[simplicity(path = "equal32.simpl", entry = "equal32")]
extern fn raw_equal32(a: u32, b: u32) -> bool;
```

Use `pub extern fn` to export it from an imported HL module. The path is relative
to the declaring `.simf` file, including when that file is imported. `path` and
`entry` are quoted strings without escape sequences, in the order shown above.
The entry must be a named root in the parsed raw forest. Use a name other than
`main` for non-unit functions: raw Simplicity constrains `main` to `1 -> 1`.

The compiler parses raw text using the selected Core or Elements jet set and
checks its finalized structural arrow against the HL signature. The input is
the HL tuple of all parameters, in declaration order (zero parameters is unit;
one parameter is that parameter's representation). The output is the declared
return type, or unit if omitted. Type aliases and compound types use the same
structural representation as ordinary HL functions; nominal HL type distinctions
are enforced at the call site.

Each call compiles its HL arguments, then composes them with the raw graph in the
same inference context. Linking preserves the raw graph's type annotations and
internal sharing. The resulting program uses the usual witness satisfaction,
pruning, serialization and execution pipeline; there is no separate runtime call
or bytecode interpreter for the fragment.

The initial version accepts closed raw fragments: witnesses, typed holes and
`disconnect` are rejected. Supply witnesses and template parameters through HL
arguments instead. Even unused external declarations are checked. Library users
must use `TemplateAst::new_with_dep` or `CompiledProgram::new_with_dep` with a
named source for relative paths. Anonymous string APIs accept absolute paths. Custom jet
hinters must implement `parse_simplicity` to opt in.

This compiler milestone reads `.simpl` files from disk. Editor dependency
tracking, unsaved raw-buffer overlays, cross-file diagnostics and navigation are
follow-up work. Flattened text uses absolute raw paths so it retains the declaring module’s
references. It does not bundle raw files and is therefore not portable to another
machine without those files. Use the string compiler API for flattened output,
which already contains resolved HL modules.

The regression comparison (debug symbols disabled, equal witnesses, pruned)
currently measures 12 program bytes and 1474 milliweight cost for the raw equality
function, versus 17 bytes and 2202 milliweight for an equivalent HL function.
These figures describe this small example, not a general performance guarantee.
