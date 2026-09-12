# VENDORED: wgpu-types 29.0.4 with relaxed wasm32 web requirements

Source: crates.io `wgpu-types` 29.0.4, copied verbatim from the cargo registry
cache on 2026-09-12. The ONLY divergence is in `Cargo.toml`: the two
`[target.'cfg(target_arch = "wasm32")'.dependencies]` entries `js-sys` and
`web-sys` require `0.3.77` instead of `0.3.85`. No Rust source changed
(`diff -r <registry>/wgpu-types-29.0.4 vendor/wgpu-types` shows only Cargo.toml
and this file).

Why: `wgpu-types` declares `std = ["js-sys?/std", "web-sys?/std"]`. That weak
feature reference makes cargo resolve the optional, wasm32-only `js-sys` into
EVERY consumer's lockfile, activated or not. `js-sys 0.3.85` pins
`wasm-bindgen = "=0.2.108"`, which cannot coexist with a consumer that pins
`wasm-bindgen = "=0.2.106"` (Cloudflare Workers built with worker-build 0.1.0).
Measured: a crate with only `wgpu-types = "29"` + `wasm-bindgen = "=0.2.106"`
fails `cargo generate-lockfile`; with this copy patched in it resolves
(js-sys 0.3.83, wasm-bindgen 0.2.106, wgpu-core/wgpu-hal 29.0.4 untouched).

The relaxed requirement is never compiled by a native host (the deps are
wasm32-only); it only changes what the lockfile may select. Consumers apply
it with `[patch.crates-io] wgpu-types = { git = "<this repo>", rev = "<rev>" }`.

Sunset: remove this directory and the consumer's `[patch]` once the
consumer's workers move off worker-build 0.1.0 / wasm-bindgen =0.2.106
(OCI Registry follow-on: worker toolchain upgrade). Review by 2026-12-11.
