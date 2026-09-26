# Continuwuity

## A community-driven [Matrix](https://matrix.org/) homeserver

[![Chat on Matrix][matrix-chat-badge]][matrix-chatroom]
[![Join the space][matrix-space-badge]][matrix-space]
[![Open Collective backers and sponsors][opencollective-contributors-badge]][opencollective]

[matrix-chat-badge]: https://img.shields.io/matrix/continuwuity%3Acontinuwuity.org?server_fqdn=matrix.continuwuity.org&fetchMode=summary&logo=matrix
[matrix-chatroom]: https://matrix.to/#/#continuwuity:continuwuity.org?via=continuwuity.org&via=ellis.link&via=explodie.org&via=matrix.org
[matrix-space-badge]: https://img.shields.io/matrix/space%3Acontinuwuity.org?server_fqdn=matrix.continuwuity.org&fetchMode=summary&logo=matrix&label=space
[matrix-space]: https://matrix.to/#/#space:continuwuity.org?via=continuwuity.org&via=ellis.link&via=explodie.org&via=matrix.org
[opencollective-contributors-badge]: https://img.shields.io/opencollective/all/continuwuity
[opencollective]: https://opencollective.com/continuwuity

[continuwuity] is a Matrix homeserver written in Rust.
It's the official community continuation of the [conduwuit](https://github.com/girlbossceo/conduwuit) homeserver.

[![forgejo.ellis.link][forge-badge]][forge-link]
[![Stars][forge-stars-badge]][forge-stars-link]
[![Issues][forge-issues-badge]][forge-issues-link]
[![Pull Requests][forge-pulls-badge]][forge-pulls-link]

[![GitHub][github-badge]][github-link] ![Stars][github-stars-badge] -
[![Codeberg][codeberg-badge]][codeberg-link] [![Stars][codeberg-stars-badge]][codeberg-stars-link] -
[![GitLab][gitlab-badge]][gitlab-link] [![Stars][gitlab-stars-badge]][gitlab-stars-link]

<div align="center">
<br>
<a href="https://opencollective.com/continuwuity" target="_blank">
<img src="https://opencollective.com/webpack/donate/button.png?color=blue" width="250"/>
</a>
</div>

[forge-badge]: https://img.shields.io/badge/Ellis%20Git-main+packages-green?style=flat&logo=forgejo&labelColor=fff
[forge-link]: https://forgejo.ellis.link/continuwuation/continuwuity
[forge-stars-badge]: https://forgejo.ellis.link/continuwuation/continuwuity/badges/stars.svg?style=flat&labelColor=fff&color=8a5cd0
[forge-stars-link]: https://forgejo.ellis.link/continuwuation/continuwuity/stars
[forge-issues-badge]: https://forgejo.ellis.link/continuwuation/continuwuity/badges/issues/open.svg?style=flat&labelColor=fff&color=8a5cd0
[forge-issues-link]: https://forgejo.ellis.link/continuwuation/continuwuity/issues?state=open
[forge-pulls-badge]: https://forgejo.ellis.link/continuwuation/continuwuity/badges/pulls/open.svg?style=flat&labelColor=fff&color=8a5cd0
[forge-pulls-link]: https://forgejo.ellis.link/continuwuation/continuwuity/pulls?state=open

[github-badge]: https://img.shields.io/badge/GitHub-mirror-blue?style=flat&logo=github&labelColor=fff&logoColor=24292f
[github-link]: https://github.com/continuwuity/continuwuity
[github-stars-badge]: https://img.shields.io/github/stars/continuwuity/continuwuity?style=flat
[gitlab-badge]:https://img.shields.io/badge/GitLab-mirror-blue?style=flat&logo=gitlab&labelColor=fff
[gitlab-link]: https://gitlab.com/continuwuity/continuwuity
[gitlab-stars-badge]: https://img.shields.io/gitlab/stars/continuwuity/continuwuity?style=flat
[gitlab-stars-link]: https://gitlab.com/continuwuity/continuwuity/-/starrers
[codeberg-badge]:https://img.shields.io/badge/Codeberg-mirror-2185D0?style=flat&logo=codeberg&labelColor=fff
[codeberg-link]: https://codeberg.org/continuwuity/continuwuity
[codeberg-stars-badge]: https://codeberg.org/continuwuity/continuwuity/badges/stars.svg?style=flat
[codeberg-stars-link]: https://codeberg.org/continuwuity/continuwuity/stars

## Why does this exist?

The original conduwuit project has been archived and is no longer maintained. Rather than letting this Rust-based Matrix homeserver disappear, a group of community contributors have forked the project to continue its development, fix outstanding issues, and add new features.

We aim to provide a stable, well-maintained alternative for current conduwuit users and welcome newcomers seeking a lightweight, efficient Matrix homeserver.

## Who are we?

We are a group of Matrix enthusiasts, developers and system administrators who have used conduwuit and believe in its potential. Our team includes both previous
contributors to the original project and new developers who want to help maintain and improve this important piece of Matrix infrastructure.

We operate as an open community project, welcoming contributions from anyone interested in improving continuwuity.

## What is Matrix?

[Matrix](https://matrix.org) is an open, federated, and extensible network for
decentralized communication. Users from any Matrix homeserver can chat with users from all
other homeservers over federation. Matrix is designed to be extensible and built on top of.
You can even use bridges such as Matrix Appservices to communicate with users outside of Matrix, like a community on Discord.

## What are the project's goals?

Continuwuity aims to:

- Maintain a stable, reliable Matrix homeserver implementation in Rust
- Improve compatibility and specification compliance with the Matrix protocol
- Fix bugs and performance issues from the original conduwuit
- Add missing features needed by homeserver administrators
- Provide comprehensive documentation and easy deployment options
- Create a sustainable development model for long-term maintenance
- Keep a lightweight, efficient codebase that can run on modest hardware

## Can I try it out?

Check out our [website](https://continuwuity.org) for installation instructions. Start with the [deployment section](https://continuwuity.org/deploying).

If you want to try it out as a user, we have some partnered homeservers you can join:
* You can head over to [https://federated.nexus](https://federated.nexus/) in your browser.
  * Hit the `Apply to Join` button. Once your request has been accepted, you will receive an email with your username and password.
  * Head over to [https://app.federated.nexus](https://app.federated.nexus/) and you can sign in there, or use any other matrix chat client you wish elsewhere.
  * Your username for matrix will be in the form of `@username:federated.nexus`, however you can simply use the `username` part to log in. Your password is your password.

* There's also [https://continuwuity.rocks/](https://continuwuity.rocks/). You can register a new account using Cinny via [this convenient link](https://app.cinny.in/register/continuwuity.rocks), or you can use Element or another matrix client *that supports registration*.

## Can I migrate my data from x?

- **Conduwuit**: Yes
- **Conduit**: No, database is now incompatible
- **Grapevine**: No, database is now incompatible
- **Dendrite**: No
- **Synapse**: No

We haven't written up a guide on migrating from incompatible homeservers yet. Reach out to us if you need to do this!

## Contribution

See our [Contributing page](CONTRIBUTING.md) for more details.

### Development flow

- Features / changes must developed in a separate branch
- For each change, create a descriptive PR
- Your code will be reviewed by one or more of the continuwuity developers
- The branch will be deployed live on multiple tester's matrix servers to shake out bugs
- Once all testers and reviewers have agreed, the PR will be merged to the main branch
- The main branch will have nightly builds deployed to users on the cutting edge
- Every week or two, a new release is cut.

The main branch is always green!

### Policy on pulling from other forks

We welcome contributions from other forks of conduwuit, subject to our review process.
When incorporating code from other forks:

- All external contributions must go through our standard PR process
- Code must meet our quality standards and pass tests
- Code changes will require testing on multiple test servers before merging
- Attribution will be given to original authors and forks
- We prioritize stability and compatibility when evaluating external contributions
- Features that align with our project goals will be given priority consideration

## Donate to us!

If you like what we're doing, consider donating to our [**Open Collective**][opencollective]!

You can also donate individually to each of the maintainers:

- Nex: https://timedout.uk/donate.html
- Jade: https://jade.ellis.link/sponsors
- Ginger: https://github.com/sponsors/gingershaped

## Contact

Join our [Matrix room](https://matrix.to/#/#continuwuity:continuwuity.org?via=continuwuity.org&via=ellis.link&via=explodie.org&via=matrix.org) and [space](https://matrix.to/#/#space:continuwuity.org?via=continuwuity.org&via=ellis.link&via=explodie.org&via=matrix.org) to chat with us about the project!

[continuwuity]: https://forgejo.ellis.link/continuwuation/continuwuity
