# Kurmanji phonemizer

Kurmanji Kurdish (Hawar alphabet) grapheme-to-phoneme conversion for the dengjen-tts speech synthesis engine.

A pure-Rust port of eSpeak-ng's `ku` rules: letters and context rules, stress (final syllable, unstressed suffixes, clause-final pronouns), cardinal numbers, a leading minus (spoken *mînus*), decimals, `%`, `$` and `/`. Output is the IPA symbol set eSpeak-ng's `ku` voice produces, one string per sentence with `,` `.` `?` `!` clause suffixes, so it drops into Piper voices trained on eSpeak-ng output.

Stress follows eSpeak-ng after [espeak-ng#2500](https://github.com/espeak-ng/espeak-ng/pull/2500), which differs from eSpeak-ng 1.52.0 for unstressed pronouns, verb-prefix words and the plural `-an`.

## Parity tests

`tests/golden*.tsv` pin expected output and run everywhere. `tests/parity.rs` diffs against a local `espeak-ng` and skips itself when its inputs are unset:

- `DENGJEN_KU_CORPUS`: a text file, one sentence per line.
- `DENGJEN_KU_SWEEP_MAX`: compare every integer up to this value.
- `DENGJEN_KU_ESPEAK_PATH`: directory holding an `espeak-ng-data` built from the current upstream `ku_rules` and `ku_list`.
- `DENGJEN_KU_MAX_MISMATCH`: mismatching corpus lines to tolerate (default 0).

Known gap: all-caps acronyms that eSpeak-ng spells letter by letter inside running text (`ISO`, `UEFA`) are read as words.
