# Contributing guide

This page is about contributing to Continuwuity. The
[development](./development.md) page may be of interest for you as well.

If you would like to work on an [issue][issues] that is not assigned, preferably
ask in the Matrix room first at [#continuwuity:continuwuity.org][continuwuity-matrix],
and comment on it.

### Linting and Formatting

It is mandatory all your changes satisfy the lints (clippy, rustc, rustdoc, etc)
and your code is formatted via the **nightly** rustfmt (`cargo +nightly fmt`). A lot of the
`rustfmt.toml` features depend on nightly toolchain. It would be ideal if they
weren't nightly-exclusive features, but they currently still are. CI's rustfmt
uses nightly.

If you need to allow a lint, please make sure it's either obvious as to why
(e.g. clippy saying redundant clone but it's actually required) or it has a
comment saying why. Do not write inefficient code for the sake of satisfying
lints. If a lint is wrong and provides a more inefficient solution or
suggestion, allow the lint and mention that in a comment.

### Running tests locally

Tests, compilation, and linting can be run with standard Cargo commands:

```bash
# Run tests
cargo test

# Check compilation
cargo check --workspace

# Run lints
cargo clippy --workspace
# Auto-fix: cargo clippy --workspace --fix --allow-staged;

# Format code (must use nightly)
cargo +nightly fmt
```

### Matrix tests

Continuwuity uses [Complement][complement] for Matrix protocol compliance testing. Complement tests are run manually by developers, and documentation on how to run these tests locally is currently being developed.

If your changes are done to fix Matrix tests, please note that in your pull request. If more Complement tests start failing from your changes, please review the logs and determine if they're intended or not.

[Sytest][sytest] is currently unsupported.

### Writing documentation

Continuwuity's website uses [`mdbook`][mdbook] and is deployed via CI using Cloudflare Pages
in the [`documentation.yml`][documentation.yml] workflow file. All documentation is in the `docs/`
directory at the top level.

To build the documentation locally:

1. Install mdbook if you don't have it already:
   ```bash
   cargo install mdbook # or cargo binstall, or another method
   ```

2. Build the documentation:
   ```bash
   mdbook build
   ```

The output of the mdbook generation is in `public/`. You can open the HTML files directly in your browser without needing a web server.

### Inclusivity and Diversity

All **MUST** code and write with inclusivity and diversity in mind. See the
[following page by Google on writing inclusive code and
documentation](https://developers.google.com/style/inclusive-documentation).

This **EXPLICITLY** forbids usage of terms like "blacklist"/"whitelist" and
"master"/"slave", [forbids gender-specific words and
phrases](https://developers.google.com/style/pronouns#gender-neutral-pronouns),
forbids ableist language like "sanity-check", "cripple", or "insane", and
forbids culture-specific language (e.g. US-only holidays or cultures).

No exceptions are allowed. Dependencies that may use these terms are allowed but
[do not replicate the name in your functions or
variables](https://developers.google.com/style/inclusive-documentation#write-around).

In addition to language, write and code with the user experience in mind. This
is software that intends to be used by everyone, so make it easy and comfortable
for everyone to use. 🏳️‍⚧️

### Variable, comment, function, etc standards

Rust's default style and standards with regards to [function names, variable
names, comments](https://rust-lang.github.io/api-guidelines/naming.html), etc
applies here.

### Creating pull requests

Please try to keep contributions to the Forgejo Instance. While the mirrors of continuwuity
allow for pull/merge requests, there is no guarantee the maintainers will see them in a timely
manner. Additionally, please mark WIP or unfinished or incomplete PRs as drafts.
This prevents us from having to ping once in a while to double check the status
of it, especially when the CI completed successfully and everything so it
*looks* done.


Direct all PRs/MRs to the `main` branch.

By sending a pull request or patch, you are agreeing that your changes are
allowed to be licenced under the Apache-2.0 licence and all of your conduct is
in line with the Contributor's Covenant, and continuwuity's Code of Conduct.

Contribution by users who violate either of these code of conducts may not have
their contributions accepted. This includes users who have been banned from
continuwuity Matrix rooms for Code of Conduct violations.

[issues]: https://forgejo.ellis.link/continuwuation/continuwuity/issues
[continuwuity-matrix]: https://matrix.to/#/#continuwuity:continuwuity.org
[complement]: https://github.com/matrix-org/complement/
[sytest]: https://github.com/matrix-org/sytest/
[mdbook]: https://rust-lang.github.io/mdBook/
[documentation.yml]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/.forgejo/workflows/documentation.yml
