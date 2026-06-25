# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/Isvane/fuzzies/compare/v0.2.1...v0.3.0) - 2026-06-25

### Fixed

- *(search) [**breaking**] fix sorting by distance bug ([#25](https://github.com/Isvane/fuzzies/pull/25))
- *(search)* prioritize Levenshtein distance over alphabetical order ([#22](https://github.com/Isvane/fuzzies/pull/22))

### Other

- add example for dictionary method ([#26](https://github.com/Isvane/fuzzies/pull/26))
- remove redundant logic ([#27](https://github.com/Isvane/fuzzies/pull/27)

## [0.2.1](https://github.com/Isvane/fuzzies/compare/v0.2.0...v0.2.1) - 2026-06-24

### Added

- *(dictionary)* add in-place text file sorting helper and tests ([#18](https://github.com/Isvane/fuzzies/pull/18))

### Other

- add GitHub Actions CI workflow for Rust ([#17](https://github.com/Isvane/fuzzies/pull/17))
- *(benches)* update benchmark ([#16](https://github.com/Isvane/fuzzies/pull/16))
- add can_match to prune dead branches ([#14](https://github.com/Isvane/fuzzies/pull/14))

## [0.2.0](https://github.com/Isvane/fuzzies/compare/v0.1.0...v0.2.0) - 2026-06-23

### Added

- *(search)* support configurable Levenshtein distance ([#10](https://github.com/Isvane/fuzzies/pull/10))
- *(search)* add dynamic limit to SearchBuilder ([#7](https://github.com/Isvane/fuzzies/pull/7))
- *(search)* [**breaking**] implement Builder pattern for dictionary search ([#3](https://github.com/Isvane/fuzzies/pull/3))

### Other

- *(dictionary)* make Dictionary fields private ([#13](https://github.com/Isvane/fuzzies/pull/13))
- add public API docstrings and update README examples ([#12](https://github.com/Isvane/fuzzies/pull/12))
- accept generic impl AsRef<Path> in Dictionary::open and build ([#9](https://github.com/Isvane/fuzzies/pull/9))
- *(build)* move build to a Dictionary ([#8](https://github.com/Isvane/fuzzies/pull/8))
- enhance README and update example ([#6](https://github.com/Isvane/fuzzies/pull/6))
- add release-plz workflow ([#4](https://github.com/Isvane/fuzzies/pull/4))
- include README in crate documentation ([#1](https://github.com/Isvane/fuzzies/pull/1))
