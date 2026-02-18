# Contributing guide

This page is about contributing to Continuwuity. The
[development](/development/index.mdx) and [code style guide](/development/code_style.mdx) pages may be of interest for you as well.

If you would like to work on an [issue][issues] that is not assigned, preferably
ask in the Matrix room first at [#continuwuity:continuwuity.org][continuwuity-matrix],
and comment on it.

### Code Style

Please review and follow the [code style guide](/development/code_style.mdx) for formatting, linting, naming conventions, and other code standards.

### Pre-commit Checks

Continuwuity uses pre-commit hooks to enforce various coding standards and catch common issues before they're committed. These checks include:

- Code formatting and linting
- Typo detection (both in code and commit messages)
- Checking for large files
- Ensuring proper line endings and no trailing whitespace
- Validating YAML, JSON, and TOML files
- Checking for merge conflicts

You can run these checks locally by installing [prefligit](https://github.com/j178/prefligit):


```bash
# Requires UV: https://docs.astral.sh/uv/getting-started/installation/
# Mac/linux: curl -LsSf https://astral.sh/uv/install.sh | sh
# Windows: powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"

# Install prefligit using cargo-binstall
cargo binstall prefligit

# Install git hooks to run checks automatically
prefligit install

# Run all checks
prefligit --all-files
```

Alternatively, you can use [pre-commit](https://pre-commit.com/):
```bash
# Requires python

# Install pre-commit
pip install pre-commit

# Install the hooks
pre-commit install

# Run all checks manually
pre-commit run --all-files
```

These same checks are run in CI via the prefligit-checks workflow to ensure consistency. These must pass before the PR is merged.

### Running tests locally

Tests, compilation, and linting can be run with standard Cargo commands:

```bash
# Run tests
cargo test

# Check compilation
cargo check --workspace --features full

# Run lints
cargo clippy --workspace --features full
# Auto-fix: cargo clippy --workspace --features full --fix --allow-staged;

# Format code (must use nightly)
cargo +nightly fmt
```

### Matrix tests

Continuwuity uses [Complement][complement] for Matrix protocol compliance testing. Complement tests are run manually by developers, and documentation on how to run these tests locally is currently being developed.

If your changes are done to fix Matrix tests, please note that in your pull request. If more Complement tests start failing from your changes, please review the logs and determine if they're intended or not.

[Sytest][sytest] is currently unsupported.

### Writing documentation

Continuwuity's website uses [`rspress`][rspress] and is deployed via CI using Cloudflare Pages
in the [`documentation.yml`][documentation.yml] workflow file. All documentation is in the `docs/`
directory at the top level.

To load the documentation locally:

1. Install NodeJS and npm from their [official website][nodejs-download] or via your package manager of choice

2. From the project's root directory, install the relevant npm modules

   ```bash
   npm ci
   ```

3. Make changes to the document pages as you see fit

4. Generate a live preview of the documentation

   ```bash
   npm run docs:dev
   ```

   A webserver for the docs will be spun up for you (e.g. at `http://localhost:3000`). Any changes you make to the documentation will be live-reloaded on the webpage.

Alternatively, you can build the documentation using `npm run docs:build` - the output of this will be in the `/doc_build` directory. Once you're happy with your documentation updates, you can commit the changes.

### Commit Messages

Continuwuity follows the [Conventional Commits](https://www.conventionalcommits.org/) specification for commit messages. This provides a standardized format that makes the commit history more readable and enables automated tools to generate changelogs.

The basic structure is:

```
<type>[(optional scope)]: <description>

[optional body]

[optional footer(s)]
```

The allowed types for commits are:
- `fix`: Bug fixes
- `feat`: New features
- `docs`: Documentation changes
- `style`: Changes that don't affect the meaning of the code (formatting, etc.)
- `refactor`: Code changes that neither fix bugs nor add features
- `perf`: Performance improvements
- `test`: Adding or fixing tests
- `build`: Changes to the build system or dependencies
- `ci`: Changes to CI configuration
- `chore`: Other changes that don't modify source or test files

Examples:
```
feat: add user authentication
fix(database): resolve connection pooling issue
docs: update installation instructions
```

The project uses the `committed` hook to validate commit messages in pre-commit. This ensures all commits follow the conventional format.

### Creating pull requests

Please try to keep contributions to the Forgejo Instance. While the mirrors of continuwuity
allow for pull/merge requests, there is no guarantee the maintainers will see them in a timely
manner. Additionally, please mark WIP or unfinished or incomplete PRs as drafts.
This prevents us from having to ping once in a while to double check the status
of it, especially when the CI completed successfully and everything so it
*looks* done.

Before submitting a pull request, please ensure:
1. Your code passes all CI checks (formatting, linting, typo detection, etc.)
2. Your code follows the [code style guide](/development/code_style.md)
3. Your commit messages follow the conventional commits format
4. Tests are added for new functionality
5. Documentation is updated if needed

Direct all PRs/MRs to the `main` branch.

By sending a pull request or patch, you are agreeing that your changes are
allowed to be licenced under the Apache-2.0 licence and all of your conduct is
in line with the Contributor's Covenant, and continuwuity's Code of Conduct.

Contribution by users who violate either of these code of conducts may not have
their contributions accepted. This includes users who have been banned from
continuwuity Matrix rooms for Code of Conduct violations.

[issues]: https://forgejo.ellis.link/continuwuation/continuwuity/issues
[continuwuity-matrix]: https://matrix.to/#/#continuwuity:continuwuity.org?via=continuwuity.org&via=ellis.link&via=explodie.org&via=matrix.org
[complement]: https://github.com/matrix-org/complement/
[sytest]: https://github.com/matrix-org/sytest/
[nodejs-download]: https://nodejs.org/en/download
[rspress]: https://rspress.rs/
[documentation.yml]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/.forgejo/workflows/documentation.yml
