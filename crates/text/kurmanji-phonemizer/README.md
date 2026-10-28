# Kurmanji phonemizer

Kurmanji Kurdish (Hawar alphabet) grapheme-to-phoneme conversion for the dengjen-tts speech synthesis engine.

A pure-Rust port of eSpeak-ng's `ku` rules: letters and context rules, stress (final syllable, unstressed suffixes, clause-final pronouns), cardinal numbers, a leading minus (spoken *mînus*), decimals, `%`, `$` and `/`. Output is the IPA symbol set eSpeak-ng's `ku` voice produces, one string per sentence with `,` `.` `?` `!` clause suffixes, so it drops into Piper voices trained on eSpeak-ng output.

Where eSpeak-ng has no rule, the crate spells the text out in Kurmanji instead of reading bare numbers or letters (`tests/divergence.tsv` pins each case):

- Clock times: `12:30` is *saet diwanzde û nîv*; quarters use *çaryek* and `12:45` is *saet yek kêm çaryek*; other minutes are *… û MM deqîqe*.
- Dates `D.M.YYYY`, `D/M/YYYY` and `YYYY-MM-DD`: `28.10.2026` is *bîst û heştê Çirîyê du hezar û bîst û şeş*.
- Ordinals with an explicit suffix (`3em`, `3-yem`, `3emîn`); a bare `3.` stays a number.
- Numbers follow the Turkish convention: `.` before exactly three digits groups thousands (`12.000` is 12000) and `,` is the decimal mark. `3,5` is *sê û nîv*, `2,25` is *du û çaryek*, and other decimals read the digits after *bêhnok* (`3,15` is *sê bêhnok yek pênc*).
- Currency `€` `£` `₺` and `TL`, units `km` `kg` `m` `cm` after a number, and the titles `Dr.` and `Prof.`.

Stress follows eSpeak-ng after [espeak-ng#2500](https://github.com/espeak-ng/espeak-ng/pull/2500), which differs from eSpeak-ng 1.52.0 for unstressed pronouns, verb-prefix words and the plural `-an`.

## Parity tests

`tests/golden*.tsv` pin expected output and run everywhere. `tests/parity.rs` diffs against a local `espeak-ng` and skips itself when its inputs are unset:

- `DENGJEN_KU_CORPUS`: a text file, one sentence per line.
- `DENGJEN_KU_SWEEP_MAX`: compare every integer up to this value.
- `DENGJEN_KU_ESPEAK_PATH`: directory holding an `espeak-ng-data` built from the current upstream `ku_rules` and `ku_list`.
- `DENGJEN_KU_MAX_MISMATCH`: mismatching corpus lines to tolerate (default 0).

Known gap: all-caps acronyms that eSpeak-ng spells letter by letter inside running text (`ISO`, `UEFA`) are read as words.
