# Contributing

Problems go into an [issue](https://github.com/sventmueller/splat/issues/new/choose), ideas and questions into
[Discussions](https://github.com/sventmueller/splat/discussions). Security problems follow the
[security policy](SECURITY.md).

## Pull requests

Small fixes can go straight to a pull request. For anything larger, describe the idea in
[Discussions](https://github.com/sventmueller/splat/discussions/new?category=ideas) first, so that no work goes into a
change Splat will not take.

- [docs/development.md](docs/development.md) explains how to build and test Splat.
- `make check` must pass. It covers formatting, clippy without warnings, all tests and the vendored libraries.
- New interface texts need a translation in every language, see [Translations](docs/development.md#translations). The tests fail
  without them.
- Keep to one change per pull request.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Splat by you, as
defined in the Apache-2.0 license, shall be dual licensed under Apache-2.0 and MIT, without any additional terms or
conditions.
