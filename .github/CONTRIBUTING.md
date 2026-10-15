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

Each phonemizer/model crate with a `fuzz/` directory has one or more `cargo-fuzz` targets. The
`fuzz-build` job in `rust-lint.yaml` builds them on every PR so they don't bit-rot.
[`fuzz.yaml`](workflows/fuzz.yaml) runs each target for two minutes every Tuesday and Friday (or on dispatch,
with a `seconds` input), carries the corpus between runs, and uploads a crash artifact when one
is found. To run a target yourself, use `cargo +nightly fuzz run <target>` from the crate that owns
it.

## Benchmarks

`crates/audio/ops` and `crates/dengjen/synth` each have `divan` benches.

- **`audio/ops`** is self-contained (synthetic samples). The `benchmarks` job in `rust-lint.yaml`
  runs it and posts the numbers to the job summary.
- **`synth`** needs real voices. [`synth-bench.yaml`](workflows/synth-bench.yaml) runs on pull
  requests that touch the synth path (`crates/dengjen/synth`, `core`, `models/piper`,
  `models/kokoro`, `crates/text/espeak-phonemizer`), `crates/audio/**`, `Cargo.lock`,
  `rust-toolchain.toml`, the fixture manifest or `run-synth-bench.sh`. It benches the merge base
  and the PR merged onto it on the same runner, with `DIVAN_SAMPLE_SIZE=3` for the Piper benches
  (Kokoro keeps its own), and writes base-vs-head tables of median times to the job summary. If a
  PR changes `bench-fixtures.json`, the base run downloads the fixtures pinned by the base's
  manifest instead of reusing the PR's. A case that is 2x or more slower than the base raises a
  warning annotation. The job is advisory and never a required check. Runs on `main` (after each
  merge that touches those paths, weekly, and manual) use `DIVAN_SAMPLE_SIZE=10` and store the raw
  output as a `bench-main-<sha>` artifact; manual runs on other refs only write the job summary.
  The compare-script unit tests run separately in `bench-script-test.yaml`.
- **Fixtures** are one Piper voice (`en_US-lessac-medium`) and one Kokoro voice (`af_bella`).
  They are archives on the `bench-fixtures-v1` release of this repo, pinned by sha256 in
  [`bench-fixtures.json`](bench-fixtures.json) and downloaded from the release on a cache miss.
  Only runs on `main` save the `actions/cache` entries, because a PR-scoped cache is invisible to
  other PRs. Merging a fixture change seeds the cache through the `main` push run; the weekly run
  does too. Until then PR runs download from the release.
- **Piper `rt`** (`encoder.onnx` + `decoder.onnx`) has no upstream export. Its tests run in
  `piper-real-voice-e2e` when `PIPER_RT_TEST_VOICE_ARCHIVE_URL` and
  `PIPER_RT_TEST_VOICE_ARCHIVE_SHA256` are set; `run-synth-bench.sh` skips
  its benches (`--skip bench_realtime`), so run them locally.

Running synth benches locally: download the release archives, extract `piper-std.tar.gz` into
`crates/dengjen/synth/models/std` and `kokoro.tar.gz` into `crates/dengjen/synth/models/kokoro`,
point `DENGJEN_ESPEAKNG_DATA_DIRECTORY` at the directory holding `espeak-ng-data`, then run
`cargo bench -p dengjen-tts --bench benchmarks -- --skip bench_realtime`. Without fixtures the
tests self-skip and the benches panic.

Changing a fixture: run `.github/scripts/build-bench-fixtures.sh OUT_DIR` with `PIPER_REV` and
`KOKORO_REV` set to the upstream commits, upload the archives to a new release tag, and update
`tag` and `sha256` in `bench-fixtures.json`. Never overwrite an existing release asset. Fixture
tags must not start with `v`, which is what keeps `notify-docs.yaml` from pinging the docs site.

CI's `piper-real-voice-e2e` and `kokoro-real-voice-e2e` jobs run the real-voice *tests* on the
weekly schedule, on manual dispatch, and on pull requests that touch `crates/dengjen`,
`crates/text`, `crates/audio`, `crates/frontends/cli`, `Cargo.lock`, `rust-toolchain.toml` or the
fixture manifest and scripts. They use the same sha256-pinned fixtures from `bench-fixtures.json`
and share the bench job's fixture cache.

Some tests return early (and pass) unless their fixture environment variable is set. CI sets only
the ones whose fixture a job already downloads:

| Variable | Test | In CI |
| --- | --- | --- |
| `DENGJEN_KOKORO_VOCAB` | `every_table_symbol_exists_in_the_kokoro_vocab` (Kokoro, `japanese` feature) | Set by `kokoro-real-voice-e2e` from the fixture's `config.json`. |
| `DENGJEN_JA_DICT_DIR` | Japanese G2P tests in `synthetic_inference.rs` | Not set. Needs the jpreprocess `naist-jdic-jpreprocess.tar.gz` (see the phonemizers docs page), which no job downloads. |
| `DENGJEN_MELOTTS_TEST_VOICE_CONFIG` | `e2e_real_voice.rs` in `dengjen-tts-melotts` | Not set. No MeloTTS fixture archive exists. Point it at a downloaded voice's `config.json` locally. |

The Python wheel tests (`crates/frontends/python/tests`) run on every PR in the `linux` and
`macos` jobs of `CI.yaml`; the Go and Java binding suites run in the `bindings-go` and
`bindings-java` jobs of `rust-lint.yaml`.

## Workspace dependencies

`crates/workspace-hack` unifies feature flags for shared dependencies across the workspace's 13
crates (via [`cargo-hakari`](https://docs.rs/cargo-hakari)), so CI jobs that check the same crate
under different feature combinations (e.g. `coverage`) don't force a rebuild of shared deps on
every switch. After adding, removing, or changing the features of a dependency, run
`cargo hakari generate` and commit the result — CI's `hakari` job fails if it's stale.

## Releasing

`prepare-release.yaml` opens the release PR with the optional `RELEASE_PR_TOKEN` repository secret
(a fine-grained PAT or GitHub App token with Contents and Pull requests write access) and falls
back to `GITHUB_TOKEN` when it is unset. A PR created with `GITHUB_TOKEN` triggers no
`pull_request` workflows, so without `RELEASE_PR_TOKEN` the release PR gets no CI and required
checks never report; a maintainer must then close and reopen it, or push to it, to start them.

Maintainers only. `Cargo.toml`'s `[workspace.package].version` is the single source of truth every
published artifact tracks in lockstep — the Rust crates, Java bindings, the `dengjen-tts-go`
mirror, the gRPC server release, and the Python wheels all track it.

1. Run the **Prepare release** workflow (`workflow_dispatch`, from the Actions tab), leaving
   `new_tag` blank. It computes the next semver version from Conventional Commit subjects merged
   since the last `vX.Y.Z` tag (`fix:`/etc → patch, `feat:` → minor, `!`/`BREAKING CHANGE:` →
   major, only docs/chore/style/refactor/test/ci/build/perf since the last tag means no release) and opens a PR
   bumping every hand-synced copy of it.
2. Review and merge that PR. **This is the release gate** — merging it releases the version in the
   diff, with nothing further to confirm: [`release.yaml`](workflows/release.yaml) tags that merge
   commit `vX.Y.Z` and runs [`publish-crates.yaml`](workflows/publish-crates.yaml) (crates.io),
   [`publish-java.yaml`](workflows/publish-java.yaml) (Maven Central),
   [`publish-go.yaml`](workflows/publish-go.yaml) (the `ZirekHQ/dengjen-tts-go` mirror),
   [`publish-grpc.yaml`](workflows/publish-grpc.yaml) (the gRPC server's GitHub Release), and
   [`publish-python.yaml`](workflows/publish-python.yaml) (wheel build) as jobs of that same run.
   `release.yaml` then uploads the wheels to PyPI (Trusted Publishing for `pydengjen`, registered
   for both `release.yaml` and `prepare-release.yaml` because PyPI rejects reusable workflows) and
   publishes one whole-repo GitHub Release once crates/java/go/grpc/python all succeed. On the
   override path below, `prepare-release.yaml` does the PyPI upload after `release.yaml` returns.
3. **Direct-release override**: setting `new_tag` (and optionally `previous_tag` — overrides the
   Java changelog baseline — and `dry_run`) on **Prepare release** skips `next-version.sh`,
   `bump-version.sh`, and the PR entirely, and hands off straight to `release.yaml` for the
   tag/publish given in `new_tag`. Because this bypasses the PR review that's normally the release
   gate, it requires approval on the `release` environment (Maintainers team) before it runs.
   **Self-approval is currently still possible** — the environment's `prevent_self_review` setting
   hasn't been flipped to `true` yet (repo Settings, tracked separately, not part of any workflow
   file); until it is, "requires approval" means a click, not necessarily a second person.

If publishing fails partway through, retry — no new tag needed either way, since every publish
step is idempotent (skips an artifact already published):
- Same version, still current on `main`: use GitHub's "Re-run failed jobs" on the original
  `release.yaml` run (Actions tab). It re-runs just the failed job(s) against that run's own
  commit, no new dispatch needed.
- Stale version (a newer version has since bumped past it on `main`): run **Prepare release**
  again, this time from the old tag (`--ref v<old-version>` on the CLI, or pick it from the
  branch/tag dropdown in the Actions tab) with `new_tag: v<old-version>` set. This is the
  direct-release override path above — it takes the version from `new_tag` directly (checked
  against that ref's own `Cargo.toml` to catch a mismatch, but not read from it), and the
  tag-push step's existing-tag branch reuses the tag rather than erroring, so it doesn't need
  `main` to still be at that version. Only works for tags
  cut *after* this override path shipped — dispatching against an older tag runs *that tag's* copy
  of these workflow files, which won't have the `new_tag` input or the `workflow_call` trigger this
  retry path depends on.

## Getting started

See [README.md](../README.md) for build instructions and [CLAUDE.md](../CLAUDE.md) for the lint
and CI conventions enforced on this repo.

## Verifying release artifacts

The gRPC server zip and the SBOM (`dengjen-tts-<version>.cdx.json`) on a GitHub release carry build
provenance attestations: `gh attestation verify <file> --repo ZirekHQ/dengjen`. The Go module
ships `SHA256SUMS` for its bundled native libraries; the release's verify job checks the fetched
module against the build, and a mismatch deletes the release's own tag.
