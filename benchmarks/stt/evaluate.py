#!/usr/bin/env python3
"""Scores Kurmanji speech-to-text models on the first N clips of the Common Voice kmr or Kobani test split.

Reports WER, CER, weight download size (MB) and CPU real-time factor per model.
"""
import argparse
import csv
import io
import json
import re
import tarfile
import time
import unicodedata
from pathlib import Path

import jiwer
import librosa
from huggingface_hub import hf_hub_download, snapshot_download

SAMPLE_RATE = 16_000
CV_REPO = "fsicoli/common_voice_22_0"
KOBANI_REPO = "aranemini/northern-kurdish-fleurs"
KOBANI_TEST_SHARDS = 2


def normalise(text):
    folded = unicodedata.normalize("NFC", text).lower().replace("’", "'")
    words = re.sub(r"[^\w\s']|_", " ", folded)
    return re.sub(r"\s+", " ", words).strip()


def load_kobani(limit):
    import pyarrow.parquet as parquet

    clips = []
    for shard in range(KOBANI_TEST_SHARDS):
        name = f"data/test-{shard:05d}-of-{KOBANI_TEST_SHARDS:05d}.parquet"
        path = hf_hub_download(KOBANI_REPO, name, repo_type="dataset")
        for row in parquet.read_table(path).to_pylist():
            audio, _ = librosa.load(io.BytesIO(row["audio"]["bytes"]), sr=SAMPLE_RATE, mono=True)
            clips.append((f"kobani-{len(clips)}", audio, row["kurdish"]))
            if len(clips) >= limit:
                return clips
    return clips


def load_common_voice(limit):
    tsv = hf_hub_download(CV_REPO, "transcript/kmr/test.tsv", repo_type="dataset")
    with open(tsv, encoding="utf-8", newline="") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    wanted = {row["path"]: row["sentence"] for row in rows[:limit]}
    tar_path = hf_hub_download(CV_REPO, "audio/kmr/test/kmr_test_0.tar", repo_type="dataset")
    return list(read_clips(tar_path, wanted))


TEST_SETS = {"cv": load_common_voice, "kobani": load_kobani}


def read_clips(tar_path, wanted):
    with tarfile.open(tar_path) as archive:
        for member in archive:
            name = Path(member.name).name
            if name in wanted:
                data = archive.extractfile(member).read()
                audio, _ = librosa.load(io.BytesIO(data), sr=SAMPLE_RATE, mono=True)
                yield name, audio, wanted[name]


def hf_ctc_transcriber(spec):
    import torch
    from transformers import AutoModelForCTC, AutoProcessor

    processor = AutoProcessor.from_pretrained(spec["id"])
    model = AutoModelForCTC.from_pretrained(spec["id"]).eval()

    def transcribe(audio):
        inputs = processor(audio, sampling_rate=SAMPLE_RATE, return_tensors="pt")
        with torch.inference_mode():
            logits = model(**inputs).logits
        return processor.batch_decode(logits.argmax(-1))[0]

    return transcribe


def hf_whisper_transcriber(spec):
    from transformers import pipeline

    asr = pipeline("automatic-speech-recognition", model=spec["id"], device=-1)
    return lambda audio: asr(
        {"raw": audio, "sampling_rate": SAMPLE_RATE},
        chunk_length_s=30,
        generate_kwargs={"max_new_tokens": 128},
    )["text"]


def sherpa_ctc_transcriber(spec):
    import sherpa_onnx

    folder = Path(snapshot_download(spec["id"], allow_patterns=[spec["model_file"], "tokens.txt"]))
    recogniser = sherpa_onnx.OfflineRecognizer.from_omnilingual_asr_ctc(
        model=str(folder / spec["model_file"]), tokens=str(folder / "tokens.txt"), num_threads=4
    )

    def transcribe(audio):
        stream = recogniser.create_stream()
        stream.accept_waveform(SAMPLE_RATE, audio)
        recogniser.decode_stream(stream)
        return stream.result.text

    return transcribe


BACKENDS = {
    "hf-ctc": hf_ctc_transcriber,
    "hf-whisper": hf_whisper_transcriber,
    "sherpa-ctc": sherpa_ctc_transcriber,
}


def download_size_mb(spec):
    from huggingface_hub import HfApi

    info = HfApi().model_info(spec["id"], files_metadata=True)
    weights = [s.size or 0 for s in info.siblings if s.rfilename.endswith((".safetensors", ".onnx"))]
    if "model_file" in spec:
        weights = [s.size or 0 for s in info.siblings if s.rfilename == spec["model_file"]]
    return round(sum(weights) / 1e6, 1)


def score(spec, clips, test_set):
    transcribe = BACKENDS[spec["backend"]](spec)
    references, hypotheses, elapsed = [], [], 0.0
    for _, audio, sentence in clips:
        started = time.perf_counter()
        hypotheses.append(normalise(transcribe(audio)))
        elapsed += time.perf_counter() - started
        references.append(normalise(sentence))
    seconds = sum(len(audio) for _, audio, _ in clips) / SAMPLE_RATE
    return {
        "model": spec["id"],
        "wer": round(100 * jiwer.wer(references, hypotheses), 2),
        "cer": round(100 * jiwer.cer(references, hypotheses), 2),
        "size_mb": download_size_mb(spec),
        "rtf_cpu": round(elapsed / seconds, 3),
        "clips": len(clips),
        "set": test_set,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", default=str(Path(__file__).with_name("models.json")))
    parser.add_argument("--set", choices=sorted(TEST_SETS), default="cv", dest="test_set")
    parser.add_argument("--limit", type=int, default=300, help="first N test clips")
    parser.add_argument("--only", help="substring filter on the model id")
    parser.add_argument("--out", default="results.json")
    args = parser.parse_args()
    if args.limit < 1:
        parser.error("--limit must be at least 1")
    specs = [m for m in json.load(open(args.models)) if not args.only or args.only in m["id"]]
    clips = TEST_SETS[args.test_set](args.limit)
    results = []
    Path(args.out).write_text("[]")
    for spec in specs:
        try:
            results.append(score(spec, clips, args.test_set))
        except Exception as error:  # one broken model must not stop the sweep
            results.append({"model": spec["id"], "error": f"{type(error).__name__}: {error}"})
        print(json.dumps(results[-1], ensure_ascii=False), flush=True)
        Path(args.out).write_text(json.dumps(results, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
