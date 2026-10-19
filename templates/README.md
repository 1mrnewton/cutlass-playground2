# Templates & golden references

A **template is just a `Document`** (see `cutlass-core`) with declared, user-
fillable slots. Because effects, text, shapes, and animation are all
serializable data interpreted by the engine — never platform code — a template
authored on one OS renders identically on all six targets.

## Files

- `*.cutlass.json` — serialized template/project documents (the portable unit).
- `golden/` — reference PNGs rendered headlessly per platform in CI; new builds
  diff against these (within a tolerance) so cross-platform drift can never be
  merged. This is the single most important safeguard for template fidelity.

## Regenerate the sample

```bash
cargo run --bin cutlass -- emit templates/demo.cutlass.json
```
