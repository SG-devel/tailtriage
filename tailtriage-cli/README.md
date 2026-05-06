# tailtriage-cli

`tailtriage-cli` is the command-line artifact loader and report emitter for `tailtriage`.

It loads captured run artifacts, validates schema compatibility, runs analysis, and emits triage reports as text or JSON.

## Installation

```bash
cargo install tailtriage-cli
```

Binary name:

```bash
tailtriage
```

## Minimal usage

```bash
tailtriage analyze tailtriage-run.json
tailtriage analyze tailtriage-run.json --format json
```

## CLI-owned artifact contract

The CLI loader validates artifact shape before analysis:

- top-level `schema_version` is required and must be a supported integer
- `requests` must exist and be non-empty
- unsupported schema versions are rejected

The non-empty `requests` rule applies to CLI artifact loading.

## Output selection

- default output is human-readable text
- `--format json` emits the structured report JSON

## Loader and lifecycle warnings

`tailtriage analyze` may emit loader/lifecycle warnings to stderr before report output.

These warnings are separate from report `warnings[]` and help explain input/collection limitations.

## Rust library users

If you need in-process analysis in Rust code, use `tailtriage-analyzer`:

- `tailtriage_analyzer::analyze_run(&Run, AnalyzeOptions)`
- `tailtriage_analyzer::render_text(&Report)`
