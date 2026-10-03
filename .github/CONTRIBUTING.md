# Contributing to Dengjen

Thanks for taking the time to contribute. Two lightweight conventions make reviews easier and
keep the project's history useful — neither is required to get a PR merged.

## Commit signing (recommended)

A signed commit lets anyone verify it actually came from you, not someone spoofing your name and
email. GitHub marks signed commits "Verified," and it's one of the cheapest supply-chain
protections available. See [GitHub's guide to commit signing](https://docs.github.com/en/authentication/managing-commit-signature-verification)
for GPG, SSH, or S/MIME setup — a few minutes, one time.

## Conventional Commit PR titles (recommended)

We squash-merge, so the PR title becomes the commit that lands on `main`. Prefixing it with a
type — `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:` — lets us auto-generate changelogs
and keeps `git log` skimmable. Example: `fix: handle empty phoneme_id_map entries in piper`. See
[conventionalcommits.org](https://www.conventionalcommits.org/) for the full spec.

Not following either convention won't block your PR — a maintainer may just tweak the title or
ask you to sign before merging.

## Fuzzing

Each phonemizer/model crate with a `fuzz/` directory has one or more `cargo-fuzz` targets. CI
only builds them (`fuzz-build` job) so they don't bit-rot — it doesn't run them, since actual
fuzzing is too slow for every PR. This is deliberate, not a gap: run one yourself with
`cargo +nightly fuzz run <target>` from the crate that owns it whenever you touch the logic that
target exercises.

## Benchmarks

`crates/audio/ops` and `crates/dengjen/synth` each have `divan` benches. CI's `benchmarks` job
runs audio/ops's bench for real and posts the numbers to the job summary — it's self-contained, no
fixtures needed. Synth's benches, and its `test_lazy_stream`/`test_parallel_stream`/
`test_realtime_stream` tests, need real Piper voice fixtures that aren't committed to this repo, so
by default the tests self-skip and CI's `clippy` job only compile-checks the benches (via
`--benches`). Running either for real, locally or in CI, needs fixtures at
`crates/dengjen/synth/models/std/model.onnx.json` and `crates/dengjen/synth/models/rt/config.json`.

CI's `piper-real-voice-e2e` job (mirroring `kokoro-real-voice-e2e`) provisions these the same
way: set the `PIPER_STD_TEST_VOICE_ARCHIVE_URL` and/or `PIPER_RT_TEST_VOICE_ARCHIVE_URL` repo
variables to `.tar.gz` URLs of a maintainer-sourced, license-vetted Piper voice export (`std/` and
`rt/` respectively), and it downloads, extracts, and runs the corresponding tests/benches on the
weekly schedule or on `workflow_dispatch`.

## Workspace dependencies

`crates/workspace-hack` unifies feature flags for shared dependencies across the workspace's 14
crates (via [`cargo-hakari`](https://docs.rs/cargo-hakari)), so CI jobs that check the same crate
under different feature combinations (e.g. `coverage`) don't force a rebuild of shared deps on
every switch. After adding, removing, or changing the features of a dependency, run
`cargo hakari generate` and commit the result — CI's `hakari` job fails if it's stale.

## Releasing

Maintainers only. `Cargo.toml`'s `[workspace.package].version` is the single source of truth every
published artifact tracks in lockstep — the Rust crates, Java bindings, the `dengjen-tts-go`
mirror, the gRPC server release, and the Python wheels all track it.

1. Run the **Prepare release** workflow (`workflow_dispatch`, from the Actions tab). Only a maintainer (admin or maintain role) can run it. It computes the next semver version from Conventional Commit subjects merged
   since the last `vX.Y.Z` tag (`fix:`/etc → patch, `feat:` → minor, `!`/`BREAKING CHANGE:` →
   major, only docs/chore/style/refactor/test since the last tag means no release) and opens a PR
   bumping every hand-synced copy of it.
2. Review, approve and squash- or rebase-merge that PR. **This is the release gate** — merging it releases the version in the
   diff, with nothing further to confirm: [`release.yml`](workflows/release.yml) tags that merge
   commit `vX.Y.Z` and runs [`publish-crates.yml`](workflows/publish-crates.yml) (crates.io),
   [`publish-java.yml`](workflows/publish-java.yml) (Maven Central),
   [`publish-go.yml`](workflows/publish-go.yml) (the `ZirekHQ/dengjen-tts-go` mirror),
   [`publish-grpc.yml`](workflows/publish-grpc.yml) (the gRPC server's GitHub Release), and
   [`publish-python.yml`](workflows/publish-python.yml) (wheel build) as jobs of that same run.
   `release.yml` then uploads the wheels to PyPI (Trusted Publishing for `pydengjen`, registered
   for both `release.yml` and `prepare-release.yml` because PyPI rejects reusable workflows) and
   publishes one whole-repo GitHub Release once crates/java/go/grpc/python all succeed. On the
   override path below, `prepare-release.yml` does the PyPI upload after `release.yml` returns.
3. CI skips the bump PR except the zizmor and Sonar jobs: every other PR-triggered job carries an
   `if:` guard on the PR author. A job-level skip reports as passing, so required checks stay
   satisfied; `[skip ci]` would leave them pending. Those two jobs run because their required
   checks only report from them.

If publishing fails partway through, retry — no new tag needed either way, since every publish
step is idempotent (skips an artifact already published):
- Same version, still current on `main`: use GitHub's "Re-run failed jobs" on the original
  `release.yml` run (Actions tab). It re-runs just the failed job(s) against that run's own
  commit, no new dispatch needed.
- Stale version (a newer version has since bumped past it on `main`): cut a patch release instead.

## Getting started

See [README.md](../README.md) for build instructions and [CLAUDE.md](../CLAUDE.md) for the lint
and CI conventions enforced on this repo.
