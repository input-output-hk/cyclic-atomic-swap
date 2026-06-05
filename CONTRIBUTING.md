# Contributing to the CANS protocol

Thanks for considering contributing and helping us on creating the CANS protocol!

The best way to contribute right now is to try things out and provide feedback,
but we also accept contributions to the documentation and to the
code itself.

This document contains guidelines to help you get started and how to make sure
your contribution gets accepted, making you our newest CANS protocol contributor.

## Communication channels

Should you have any questions or need some help in getting set up, you can use
these communication channels to reach the CANS protocol team and get answers in a way
where others can benefit from it as well:

- [Discord](https://discord.com/invite/56EQWW4rzv)
- [Contribute to IOG Research](https://www.iog.io/contact?form=contribute-to-iog-research) (contact form)

## Your first contribution

Contributing to the documentation, its translation, reporting bugs or proposing features are awesome ways to get started.

## Making changes

When contributing code, it helps to have discussed the rationale and (ideally)
how something is implemented in a feature idea or bug ticket beforehand.

### Building & Testing

With the latest [Rust](https://www.rust-lang.org/tools/install) build tools installed, 
at the rootof the repository, or in each `swap-` folder, run: `make`. 
This will build the libraries, executables and run tests. 
More information is available in the `README.md` files of each folder 
(see list of them [here](https://www.rust-lang.org/tools/install)).

Besides these general build instructions, some components might document
additional steps and useful tools in their `README.md` files.

### Coding standards

Make sure to follow the Rust [Coding Standards](https://doc.rust-lang.org/style-guide/).

### Creating a pull request

Thank you for contributing, your changes by opening a pull request!
To get something merged, we usually require:

- Description of the changes – if your commit messages are great, this is less important
- Change is related to an issue, feature (idea) or bug report – ideally discussed beforehand
- Well-scoped - we prefer multiple PRs, rather than a big one
- All your commits must be [signed](https://docs.github.com/en/authentication/managing-commit-signature-verification/signing-commits) to be merged in the `main` branch

### Versioning & Changelog

During development

- Make sure `CHANGELOG.md` is kept up to date with a high-level, technical, but user-focused list of changes according to [keepachangelog](https://keepachangelog.com/en/1.0.0/)
- Bump `UNRELEASED` version in `CHANGELOG.md` according to [semver](https://semver.org/)
- All `swap-` packages are versioned the same, at latest on release their versions are aligned.
- Other packages are versioned independently of `swap-` packages and keep a dedicated changelog.

### Releasing

To perform a release

- Replace `UNRELEASED` with a date in [ISO8601](https://en.wikipedia.org/wiki/ISO_8601)
- Create a signed, annotated git tag of the version: `git tag -as <version>`
- (ideally) Use the released changes as annotation
