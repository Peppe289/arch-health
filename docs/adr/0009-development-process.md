# ADR-0009: Development process

**Status:** Accepted

## Decision
- `main` always builds and passes tests. Work happens on feature branches
  merged through pull requests.
- Commit subjects: `type(component): title` (types: `feat`, `fix`,
  `refactor`, `docs`, `test`, `build`, `ci`, `chore`). Changelog generated
  from history (`git-cliff`) when releases start.
- Quality gate: `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test`.
- Order of delivery: GitHub → CI (GitHub Actions in an `archlinux` container,
  plus `cargo-deny`) → packaging (PKGBUILD).
- Semantic versioning once the first release is tagged.
- Requirements live in `docs/requirements.md`; decisions in `docs/adr/`.
