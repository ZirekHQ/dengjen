# Kurmanji STT benchmark

Scores existing Hugging Face Kurmanji speech-to-text models by WER and CER on CPU.

```sh
pip install --require-hashes -r benchmarks/stt/requirements.txt
python benchmarks/stt/evaluate.py --set cv --limit 300 --out results.json
```

`--set` is `cv` (Common Voice 22, `kmr` test split) or `kobani` (FLEURS-style
[northern-kurdish-fleurs](https://huggingface.co/datasets/aranemini/northern-kurdish-fleurs) test split).
`requirements.txt` is the hashed lock of `requirements.in` (`uv pip compile --universal --generate-hashes`).
`--only <substring>` filters models; `models.json` lists them. The `STT benchmark` workflow runs both
sets on ubuntu and macOS via `workflow_dispatch`.

Measured 2026-10-15 on GitHub-hosted ubuntu-latest and macos-latest runners (first 300 clips).
RTF ranges span the two runners.

## Results (Common Voice)

| Model | WER % | CER % | Size MB | RTF |
|---|---|---|---|---|
| samil24/whisper-small-kurmanji-v10 | 9.6 | 2.8 | 967 | 0.9–1.7 |
| Akashpb13/xlsr_kurmanji_kurdish | 35.9 | 9.5 | 1262 | 0.2 |
| omnilingual-asr 1B CTC int8 (sherpa-onnx) | 37.7 | 12.5 | 1032 | 0.4 |
| omnilingual-asr 300M CTC int8 (sherpa-onnx) | 52.7 | 18.0 | 365 | 0.17 |
| umit/w2v-bert-kmr-common-voice | 98.6 | 96.9 | 2322 | 0.3–0.4 |
| amedcj/kurmanji-asr-model | 119.9 | 315 | 378 | 0.1 |
| amedcj/whisper-kurmanji | 561 | 481 | 967 | 3.4–4.1 |

## Results (Kobani)

| Model | WER % | CER % |
|---|---|---|
| samil24/whisper-small-kurmanji-v10 | 48.4 | 16.1 |
| omnilingual-asr 300M CTC int8 | 53.6 | 15.8 |
| Akashpb13/xlsr_kurmanji_kurdish | 54.9 | 17.5 |
| umit/w2v-bert-kmr-common-voice | 99.4 | 99.2 |
| amedcj/kurmanji-asr-model | 119.1 | 220.8 |
| amedcj/whisper-kurmanji | 118.6 | 50.2 |

The omnilingual 1B model has no Kobani score; the scoring step timed out.
`samil24` was scored on one runner only.

## Reading the numbers

- `samil24` falls from 9.6% (CV) to 48.4% (Kobani), which hints that its training data overlaps the
  Common Voice test split (unverified). Prefer the Kobani figure.
- No existing model reaches a usable out-of-domain WER, so fine-tuning is likely needed. Among
  the models with usable WER, omnilingual 300M is the smallest and fastest.
- `salamhasan/wav2vec2-large-xlsr-kurmanji` is gated and unscored.
- RTF is from GitHub-hosted runners, not phones.
- The Kobani dataset card declares no licence; confirm terms before redistributing derived models.
