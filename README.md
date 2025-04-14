# continuwuity

<!-- ANCHOR: catchphrase -->

## A community-driven [Matrix](https://matrix.org/) homeserver in Rust

<!-- ANCHOR_END: catchphrase -->

[continuwuity] is a Matrix homeserver written in Rust.
It's a community continuation of the [conduwuit](https://github.com/girlbossceo/conduwuit) homeserver. 

<!-- ANCHOR: body -->


### Why does this exist?

The original conduwuit project has been archived and is no longer maintained. Rather than letting this Rust-based Matrix homeserver disappear, a group of community contributors have forked the project to continue its development, fix outstanding issues, and add new features.

We aim to provide a stable, well-maintained alternative for current Conduit users and welcome newcomers seeking a lightweight, efficient Matrix homeserver.

### Who are we?

We are a group of Matrix enthusiasts, developers and system administrators who have used conduwuit and believe in its potential. Our team includes both previous
contributors to the original project and new developers who want to help maintain and improve this important piece of Matrix infrastructure.

We operate as an open community project, welcoming contributions from anyone interested in improving continuwuity.

### What is Matrix?

[Matrix](https://matrix.org) is an open, federated, and extensible network for
decentralized communication. Users from any Matrix homeserver can chat with users from all
other homeservers over federation. Matrix is designed to be extensible and built on top of.
You can even use bridges such as Matrix Appservices to communicate with users outside of Matrix, like a community on Discord.

### What are the project's goals?

Continuwuity aims to:

- Maintain a stable, reliable Matrix homeserver implementation in Rust
- Improve compatibility and specification compliance with the Matrix protocol
- Fix bugs and performance issues from the original conduwuit
- Add missing features needed by homeserver administrators
- Provide comprehensive documentation and easy deployment options
- Create a sustainable development model for long-term maintenance
- Keep a lightweight, efficient codebase that can run on modest hardware

### Can I try it out?

Not right now. We've still got work to do!


### What are we working on?

We're working our way through all of the issues in the [Forgejo project](https://forgejo.ellis.link/continuwuation/continuwuity/issues).

- [Replacing old conduwuit links with working continuwuity links](https://forgejo.ellis.link/continuwuation/continuwuity/issues/742)
- [Getting CI and docs deployment working on the new Forgejo project](https://forgejo.ellis.link/continuwuation/continuwuity/issues/740)
- [Packaging & availability in more places](https://forgejo.ellis.link/continuwuation/continuwuity/issues/747)
- [Appservices bugs & features](https://forgejo.ellis.link/continuwuation/continuwuity/issues?q=&type=all&state=open&labels=178&milestone=0&assignee=0&poster=0)
- [Improving compatibility and spec compliance](https://forgejo.ellis.link/continuwuation/continuwuity/issues?labels=119)
- Automated testing
- [Admin API](https://forgejo.ellis.link/continuwuation/continuwuity/issues/748)
- [Policy-list controlled moderation](https://forgejo.ellis.link/continuwuation/continuwuity/issues/750)

### Can I migrate my data from x?

- Conduwuit: Yes
- Conduit: No, database is now incompatible
- Grapevine: No, database is now incompatible
- Dendrite: No
- Synapse: No

Although you can't migrate your data from other homeservers, it is perfectly acceptable to set up continuwuity on the same domain as a previous homeserver.


<!-- ANCHOR_END: body -->

## Contribution

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

<!-- ANCHOR: footer -->

#### Contact

<!-- TODO: contact details -->

<!-- ANCHOR_END: footer -->


[continuwuity]: https://forgejo.ellis.link/continuwuation/continuwuity
