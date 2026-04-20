# Documentation map

## Start here

- **Fastest first run from this repo:** [`user-guide.md#path-a--run-from-this-repo-workspace`](user-guide.md#path-a--run-from-this-repo-workspace)
- **Use published crates in external projects:** [`user-guide.md#path-b--use-published-crates-from-cratesio`](user-guide.md#path-b--use-published-crates-from-cratesio)
- **Split lifecycle API contract (`StartedRequest`, `RequestHandle`, `RequestCompletion`):** [`user-guide.md#request-lifecycle-correctness-required`](user-guide.md#request-lifecycle-correctness-required)
- **Live arm/disarm controller guide:** [`../tailtriage-controller/README.md`](../tailtriage-controller/README.md)
- **Axum adapter usage (`TailtriageRequest` + middleware):** [`user-guide.md#axum-adapter-surface-optional`](user-guide.md#axum-adapter-surface-optional)
- **Public examples:** [`../tailtriage-tokio/examples/`](../tailtriage-tokio/examples/) and [`../tailtriage-axum/examples/`](../tailtriage-axum/examples/)
- **Demo walkthrough and recommended first three demos:** [`getting-started-demo.md`](getting-started-demo.md)
- **How to read diagnosis output:** [`diagnostics.md`](diagnostics.md)

## Reference

- **Architecture and crate responsibilities:** [`architecture.md`](architecture.md)
- **Collector-limits measurement path (stress matrix + measured operating guidance):** [`collector-limits.md`](collector-limits.md)
- **Runtime-cost attribution path (baked-in/core/sampler/drop-path):** [`runtime-cost.md`](runtime-cost.md)
