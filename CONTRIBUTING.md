# Contributing guide

This page is for about contributing to conduwuit. The
[development](./development.md) page may be of interest for you as well.

If you would like to work on an [issue][issues] that is not assigned, preferably
ask in the Matrix room first at [#continuwuity:continuwuity.org][continuwuity-matrix],
and comment on it.

### Linting and Formatting

It is mandatory all your changes satisfy the lints (clippy, rustc, rustdoc, etc)
and your code is formatted via the **nightly** `cargo fmt`. A lot of the
`rustfmt.toml` features depend on nightly toolchain. It would be ideal if they
weren't nightly-exclusive features, but they currently still are. CI's rustfmt
uses nightly.

If you need to allow a lint, please make sure it's either obvious as to why
(e.g. clippy saying redundant clone but it's actually required) or it has a
comment saying why. Do not write inefficient code for the sake of satisfying
lints. If a lint is wrong and provides a more inefficient solution or
suggestion, allow the lint and mention that in a comment.

### Running CI tests locally

continuwuity's CI for tests, linting, formatting, audit, etc use
[`engage`][engage]. engage can be installed from nixpkgs or `cargo install
engage`. continuwuity's Nix flake devshell has the nixpkgs engage with `direnv`.
Use `engage --help` for more usage details.

To test, format, lint, etc that CI would do, install engage, allow the `.envrc`
file using `direnv allow`, and run `engage`.

All of the tasks are defined at the [engage.toml][engage.toml] file. You can
view all of them neatly by running `engage list`

If you would like to run only a specific engage task group, use `just`:

- `engage just <group>`
- Example: `engage just lints`

If you would like to run a specific engage task in a specific group, use `just
<GROUP> [TASK]`: `engage just lints cargo-fmt`

The following binaries are used in [`engage.toml`][engage.toml]:

- [`engage`][engage]
- `nix`
- [`direnv`][direnv]
- `rustc`
- `cargo`
- `cargo-fmt`
- `rustdoc`
- `cargo-clippy`
- [`cargo-audit`][cargo-audit]
- [`cargo-deb`][cargo-deb]
- [`lychee`][lychee]
- [`markdownlint-cli`][markdownlint-cli]
- `dpkg`

### Matrix tests

CI runs [Complement][complement], but currently does not fail if results from
the checked-in results differ with the new results. If your changes are done to
fix Matrix tests, note that in your pull request. If more Complement tests start
failing from your changes, please review the logs (they are uploaded as
artifacts) and determine if they're intended or not.

If you'd like to run Complement locally using Nix, see the
[testing](development/testing.md) page.

[Sytest][sytest] support will come soon.

### Writing documentation

conduwuit's website uses [`mdbook`][mdbook] and deployed via CI using GitHub
Pages in the [`documentation.yml`][documentation.yml] workflow file with Nix's
mdbook in the devshell. All documentation is in the `docs/` directory at the top
level. The compiled mdbook website is also uploaded as an artifact.

To build the documentation using Nix, run: `bin/nix-build-and-cache just .#book`

The output of the mdbook generation is in `result/`. mdbooks can be opened in
your browser from the individual HTML files without any web server needed.

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

Contribution by users who violate either of these code of conducts will not have
their contributions accepted. This includes users who have been banned from
continuwuityMatrix rooms for Code of Conduct violations.

[issues]: https://forgejo.ellis.link/continuwuation/continuwuity/issues
[continuwuity-matrix]: https://matrix.to/#/#continuwuity:continuwuity.org
[complement]: https://github.com/matrix-org/complement/
[engage.toml]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/engage.toml
[engage]: https://charles.page.computer.surgery/engage/
[sytest]: https://github.com/matrix-org/sytest/
[cargo-deb]: https://github.com/kornelski/cargo-deb
[lychee]: https://github.com/lycheeverse/lychee
[markdownlint-cli]: https://github.com/igorshubovych/markdownlint-cli
[cargo-audit]: https://github.com/RustSec/rustsec/tree/main/cargo-audit
[direnv]: https://direnv.net/
[mdbook]: https://rust-lang.github.io/mdBook/
[documentation.yml]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/.forgejo/workflows/documentation.yml
