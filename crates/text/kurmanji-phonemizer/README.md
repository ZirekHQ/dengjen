# Kurmanji phonemizer

Kurmanji Kurdish (Hawar alphabet) grapheme-to-phoneme conversion for the dengjen-tts speech synthesis engine.

A pure-Rust port of eSpeak-ng's `ku` rules: letters and context rules, stress (final syllable, unstressed suffixes, clause-final pronouns), cardinal numbers, a leading minus (spoken *mînus*), decimals, `%`, `$` and `/`. Output is the IPA symbol set eSpeak-ng's `ku` voice produces, one string per sentence with `,` `.` `?` `!` clause suffixes, so it drops into Piper voices trained on eSpeak-ng output.

Where eSpeak-ng has no rule, the crate spells the text out in Kurmanji instead of reading bare numbers or letters (`tests/divergence.tsv` pins each case):

- Clock times: `12:30` is *saet diwanzde û nîv*; quarters use *çaryek* and `12:45` is *saet yek kêm çaryek*; other minutes are *… û MM deqîqe*, and `12:10:15` adds *û panzde saniye*.
- Dates `D.M.YYYY`, `D/M/YYYY` and `YYYY-MM-DD`: `28.10.2026` is *bîst û heştê Çirîyê du hezar û bîst û şeş*.
- Ordinals with an explicit suffix (`3em`, `3-yem`, `3emîn`); a bare `3.` stays a number.
- Numbers follow the Turkish convention: `.` before exactly three digits groups thousands (`12.000` is 12000) and `,` is the decimal mark. `3,5` is *sê û nîv*, `2,25` is *du û çaryek*, and other decimals read the digits after *bêhnok* (`3,15` is *sê bêhnok yek pênc*).
- Currency `€` `£` `₺` and `TL`, units `km` `kg` `m` `cm` after a number (`3,5 kg` is *sê kîlogram û nîv*), and the titles `Dr.` and `Prof.`.

Stress follows eSpeak-ng after [espeak-ng#2500](https://github.com/espeak-ng/espeak-ng/pull/2500), which differs from eSpeak-ng 1.52.0 for unstressed pronouns, verb-prefix words and the plural `-an`.

## Parity tests

`tests/golden*.tsv` pin expected output and run everywhere. The parity tests live in `dengjen-espeak-phonemizer` (`tests/kurmanji_parity.rs`) because they call eSpeak-ng's runtime phonemizer, the call Piper uses, not the `espeak-ng` command line, which forces stress on a lone unstressed word. CI runs them against the data built from the pinned eSpeak-ng source (`scripts/stage-espeak-data.sh`), over `tests/kurmanji_corpus.txt` (Common Voice sentences, CC0) and the integers 0 to 3000. Locally:

```sh
bash scripts/stage-espeak-data.sh /tmp/espeak
DENGJEN_ESPEAKNG_DATA_DIRECTORY=/tmp/espeak cargo test -p dengjen-espeak-phonemizer --test kurmanji_parity -- --test-threads=1 --include-ignored
```

- `DENGJEN_KU_CORPUS`: another corpus to compare, a text file with one sentence per line.
- `DENGJEN_KU_CORPUS_COLUMN`: a 1-based column number. Treats that corpus as a TSV with a header row and compares that column, for example `4` for the `sentence` column of a Common Voice `test.tsv`. Lines are sorted and deduplicated.
- `DENGJEN_KU_MAX_MISMATCH`: mismatching corpus lines to tolerate (default 0).

Intended differences, skipped by the parity tests: `Dr.` is read as *doktor* where eSpeak-ng spells *D R*, and `s` plus a combining cedilla is read as `ş` where eSpeak-ng drops the mark. eSpeak-ng also returns a bare `.` sentence for text that is only quote marks; this crate returns none.

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
