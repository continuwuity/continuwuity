---
name: 'New pull request'
about: 'Open a new pull request to contribute to continuwuity'
ref: 'main'
---

<!--
In order to help reviewers know what your pull request does at a glance, you should ensure that

1. Your PR title is a short, single sentence describing what you changed
2. You have described in more detail what you have changed, why you have changed it, what the
   intended effect is, and why you think this will be beneficial to the project.

If you have made any potentially strange/questionable design choices, but didn't feel they'd benefit
from code comments, please don't mention them here - after opening your pull request,
go to "files changed", and click on the "+" symbol in the line number gutter,
and attach comments to the lines that you think would benefit from some clarification.
-->

This pull request...

<!-- Example:
This pull request allows us to warp through time and space ten times faster than before by
double-inverting the warp drive with hyperheated jump fluid, both making the drive faster and more
efficient. This resolves the common issue where we have to wait more than 10 milliseconds to
engage, use, and disengage the warp drive when travelling between galaxies.
-->

<!-- Closes: #... -->
<!-- Fixes: #...  -->
<!-- Uncomment the above line(s) if your pull request fixes an issue or closes another pull request
by superseding it. Replace `#...` with the issue/pr number, such as `#123`. -->

**Pull request checklist:**

<!-- You need to complete these before your PR can be considered.
If you aren't sure about some, feel free to ask for clarification in #dev:continuwuity.org. -->
- [ ] This pull request targets the `main` branch, and the branch is named something other than
      `main`.
- [ ] I have written an appropriate pull request title and my description is clear.
- [ ] I understand I am responsible for the contents of this pull request.
- I have followed the [contributing guidelines][c1]:
  - [ ] My contribution follows the [code style][c2], if applicable.
  - [ ] I ran [pre-commit checks][c1pc] before opening/drafting this pull request.
  - [ ] I have [tested my contribution][c1t] (or proof-read it for documentation-only changes)
        myself, if applicable. This includes ensuring code compiles.
  - [ ] My commit messages follow the [commit message format][c1cm] and are descriptive.
  - [ ] I have written a [news fragment][n1] for this PR, if applicable<!--(can be done after hitting open!)-->.

<!--
Notes on these requirements:

- While not required, we encourage you to sign your commits with GPG or SSH to attest the
  authenticity of your changes.
- While we allow LLM-assisted contributions, we do not appreciate contributions that are
  low quality, which is typical of machine-generated contributions that have not had a lot of love
  and care from a human. Please do not open a PR if all you have done is asked ChatGPT to tidy up
  the codebase with a +-100,000 diff.
- In the case of code style violations, reviewers may leave review comments/change requests
  indicating what the ideal change would look like. For example, a reviewer may suggest you lower
  a log level, or use `match` instead of `if/else` etc.
- In the case of code style violations, pre-commit check failures, minor things like typos/spelling
  errors, and in some cases commit format violations, reviewers may modify your branch directly,
  typically by making changes and adding a commit. Particularly in the latter case, a reviewer may
  rebase your commits to squash "spammy" ones (like "fix", "fix", "actually fix"), and reword
  commit messages that don't satisfy the format.
- Pull requests MUST pass the `Checks` CI workflows to be capable of being merged. This can only be
  bypassed in exceptional circumstances.
  If your CI flakes, let us know in matrix:r/dev:continuwuity.org.
- Pull requests have to be based on the latest `main` commit before being merged. If the main branch
  changes while you're making your changes, you should make sure you rebase on main before
  opening a PR. Your branch will be rebased on main before it is merged if it has fallen behind.
- We typically only do fast-forward merges, so your entire commit log will be included. Once in
  main, it's difficult to get out cleanly, so put on your best dress, smile for the cameras!
-->

[c1]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/CONTRIBUTING.md
[c2]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/docs/development/code_style.mdx
[c1pc]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/CONTRIBUTING.md#pre-commit-checks
[c1t]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/CONTRIBUTING.md#running-tests-locally
[c1cm]: https://forgejo.ellis.link/continuwuation/continuwuity/src/branch/main/CONTRIBUTING.md#commit-messages
[n1]: https://towncrier.readthedocs.io/en/stable/tutorial.html#creating-news-fragments
