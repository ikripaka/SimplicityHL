# Unstable features

Unstable features are experimental compiler capabilities. They may change
or be removed before stabilization.

## Viewing available unstable features

Run `simc --help`; the features are listed under the `-Z` flag.

## Enabling an unstable feature

Pass `-Z <feature-name>` to `simc`.

## `simplicity`

An external function can call a closed raw Simplicity entry from a disk file:

```rust
#[simplicity(path = "equal32.simpl", entry = "equal32")]
extern fn raw_equal32(a: u32, b: u32) -> bool;
```

The raw path is relative to the declaring HL file. The file must remain
available when the program is analyzed, including when flattened source is
compiled later. The compiler validates even unused declarations. It compares
the finalized entry arrow to the structural encoding of the HL parameter tuple
and return type. Zero parameters or an omitted return type encode unit; one
parameter uses its own representation, and multiple parameters use the usual
tuple layout. Structural shape matters, though equivalent encodings such as
`(2^8 * 2^8)` and `u16` are accepted at this boundary. HL calls still obey HL
type rules, including nominal enum types and literal range checks.

For example, a declaration returning `u32` for an entry returning `2` reports
an output mismatch with the path, entry, declared HL types, and expected and
found structural arrows. Inferred raw types are finalized independently of the
HL declaration; annotate the raw entry if inference chooses an unintended type.
File, parse, missing-entry, and unsupported-node failures are reported separately.

Rust consumers can inspect the diagnostics returned by `TemplateAst::new_with_dep`
or `TemplateAst::new_with_unstable`:

```rust,ignore
for diagnostic in errors.diagnostics() {
    if let simplicityhl::error::Error::SimplicityTypeMismatch {
        path, entry, expected_input, expected_output, found_input, found_output,
    } = diagnostic.error() {
        // Inspect the declaration and both structural types.
    }
}
```

## `raw_hash`

`raw_hash::<T>(tuple)` hashes a tuple of unsigned integers with SHA-256:

```rust
let digest: u256 = raw_hash::<(u8, u256)>((0x02, public_key));
```

This is the same as:

```rust
let ctx: Ctx8 = jet::sha_256_ctx_8_init();
let ctx: Ctx8 = jet::sha_256_ctx_8_add_1(ctx, 0x02);
let ctx: Ctx8 = jet::sha_256_ctx_8_add_32(ctx, public_key);
let digest: u256 = jet::sha_256_ctx_8_finalize(ctx);
```

In particular, the type parameter selects which `add_N` jets run; the digest does not
commit to it. `raw_hash::<(u8, u8)>((0x01, 0x02))` and `raw_hash::<(u16,)>((0x0102,))`
produce the same digest, as does any other type with the same bytes, such as an alias
of `u32` versus a plain `u32`. If your protocol needs the digest to be unambiguous,
commit to the type yourself, for example by hashing a domain-separation tag first.

## Adding or stabilizing a feature

See the rustdoc on `UnstableFeature` in `src/unstable.rs`; the procedures
live next to the code they mutate, so they cannot drift.
