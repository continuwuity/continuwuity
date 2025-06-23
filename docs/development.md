# Development

Information about developing the project. If you are only interested in using
it, you can safely ignore this page. If you plan on contributing, see the
[contributor's guide](./contributing.md).

## Continuwuity project layout

Continuwuity uses a collection of sub-crates, packages, or workspace members
that indicate what each general area of code is for. All of the workspace
members are under `src/`. The workspace definition is at the top level / root
`Cargo.toml`.

The crate names are generally self-explanatory:
- `admin` is the admin room
- `api` is the HTTP API, Matrix C-S and S-S endpoints, etc
- `core` is core Continuwuity functionality like config loading, error definitions,
global utilities, logging infrastructure, etc
- `database` is RocksDB methods, helpers, RocksDB config, and general database definitions,
utilities, or functions
- `macros` are Continuwuity Rust [macros][macros] like general helper macros, logging
and error handling macros, and [syn][syn] and [procedural macros][proc-macro]
used for admin room commands and others
- `main` is the "primary" sub-crate. This is where the `main()` function lives,
tokio worker and async initialisation, Sentry initialisation, [clap][clap] init,
and signal handling. If you are adding new [Rust features][features], they *must*
go here.
- `router` is the webserver and request handling bits, using axum, tower, tower-http,
hyper, etc, and the [global server state][state] to access `services`.
- `service` is the high-level database definitions and functions for data,
outbound/sending code, and other business logic such as media fetching.

It is highly unlikely you will ever need to add a new workspace member, but
if you truly find yourself needing to, we recommend reaching out to us in
the Matrix room for discussions about it beforehand.

The primary inspiration for this design was apart of hot reloadable development,
to support "Continuwuity as a library" where specific parts can simply be swapped out.
There is evidence Conduit wanted to go this route too as `axum` is technically an
optional feature in Conduit, and can be compiled without the binary or axum library
for handling inbound web requests; but it was never completed or worked.

See the Rust documentation on [Workspaces][workspaces] for general questions
and information on Cargo workspaces.

## Adding compile-time [features][features]

If you'd like to add a compile-time feature, you must first define it in
the `main` workspace crate located in `src/main/Cargo.toml`. The feature must
enable a feature in the other workspace crate(s) you intend to use it in. Then
the said workspace crate(s) must define the feature there in its `Cargo.toml`.

So, if this is adding a feature to the API such as `woof`, you define the feature
in the `api` crate's `Cargo.toml` as `woof = []`. The feature definition in `main`'s
`Cargo.toml` will be `woof = ["conduwuit-api/woof"]`.

The rationale for this is due to Rust / Cargo not supporting
["workspace level features"][9], we must make a choice of; either scattering
features all over the workspace crates, making it difficult for anyone to add
or remove default features; or define all the features in one central workspace
crate that propagate down/up to the other workspace crates. It is a Cargo pitfall,
and we'd like to see better developer UX in Rust's Workspaces.

Additionally, the definition of one single place makes "feature collection" in our
Nix flake a million times easier instead of collecting and deduping them all from
searching in all the workspace crates' `Cargo.toml`s. Though we wouldn't need to
do this if Rust supported workspace-level features to begin with.

## List of forked dependencies

During Continuwuity (and prior projects) development, we have had to fork some dependencies to support our use-cases.
These forks exist for various reasons including features that upstream projects won't accept,
faster-paced development, Continuwuity-specific usecases, or lack of time to upstream changes.

All forked dependencies are maintained under the [continuwuation organization on Forgejo](https://forgejo.ellis.link/continuwuation):

- [ruwuma][continuwuation-ruwuma] - Fork of [ruma/ruma][ruma] with various performance improvements, more features and better client/server interop
- [rocksdb][continuwuation-rocksdb] - Fork of [facebook/rocksdb][rocksdb] via [`@zaidoon1`][8] with liburing build fixes and GCC debug build fixes
- [jemallocator][continuwuation-jemallocator] - Fork of [tikv/jemallocator][jemallocator] fixing musl builds, suspicious code,
  and adding support for redzones in Valgrind
- [rustyline-async][continuwuation-rustyline-async] - Fork of [zyansheep/rustyline-async][rustyline-async] with tab completion callback
  and `CTRL+\` signal quit event for Continuwuity console CLI
- [rust-rocksdb][continuwuation-rust-rocksdb] - Fork of [rust-rocksdb/rust-rocksdb][rust-rocksdb] fixing musl build issues,
  removing unnecessary `gtest` include, and using our RocksDB and jemallocator forks
- [tracing][continuwuation-tracing] - Fork of [tokio-rs/tracing][tracing] implementing `Clone` for `EnvFilter` to
  support dynamically changing tracing environments

## Debugging with `tokio-console`

[`tokio-console`][7] can be a useful tool for debugging and profiling. To make a
`tokio-console`-enabled build of Continuwuity, enable the `tokio_console` feature,
disable the default `release_max_log_level` feature, and set the `--cfg
tokio_unstable` flag to enable experimental tokio APIs. A build might look like
this:

```bash
RUSTFLAGS="--cfg tokio_unstable" cargo +nightly build \
    --release \
    --no-default-features \
    --features=systemd,element_hacks,gzip_compression,brotli_compression,zstd_compression,tokio_console
```

You will also need to enable the `tokio_console` config option in Continuwuity when
starting it. This was due to tokio-console causing gradual memory leak/usage
if left enabled.

## Building Docker Images

To build a Docker image for Continuwuity, use the standard Docker build command:

```bash
docker build -f docker/Dockerfile .
```

The image can be cross-compiled for different architectures.

[continuwuation-ruwuma]: https://forgejo.ellis.link/continuwuation/ruwuma
[continuwuation-rocksdb]: https://forgejo.ellis.link/continuwuation/rocksdb
[continuwuation-jemallocator]: https://forgejo.ellis.link/continuwuation/jemallocator
[continuwuation-rustyline-async]: https://forgejo.ellis.link/continuwuation/rustyline-async
[continuwuation-rust-rocksdb]: https://forgejo.ellis.link/continuwuation/rust-rocksdb
[continuwuation-tracing]: https://forgejo.ellis.link/continuwuation/tracing

[ruma]: https://github.com/ruma/ruma/
[rocksdb]: https://github.com/facebook/rocksdb/
[jemallocator]: https://github.com/tikv/jemallocator/
[rustyline-async]: https://github.com/zyansheep/rustyline-async/
[rust-rocksdb]: https://github.com/rust-rocksdb/rust-rocksdb/
[tracing]: https://github.com/tokio-rs/tracing/

[7]: https://docs.rs/tokio-console/latest/tokio_console/
[8]: https://github.com/zaidoon1/
[9]: https://github.com/rust-lang/cargo/issues/12162
[workspaces]: https://doc.rust-lang.org/cargo/reference/workspaces.html
[macros]: https://doc.rust-lang.org/book/ch19-06-macros.html
[syn]: https://docs.rs/syn/latest/syn/
[proc-macro]: https://doc.rust-lang.org/reference/procedural-macros.html
[clap]: https://docs.rs/clap/latest/clap/
[features]: https://doc.rust-lang.org/cargo/reference/features.html
[state]: https://docs.rs/axum/latest/axum/extract/struct.State.html
