"""Synthesizes speech with pydengjen.

Usage: python synthesize.py [VOICE_MANIFEST_JSON] [--mode file|lazy|parallel|batched|streamed] [options]
Run with --help for every option.

The manifest path may also come from DENGJEN_EXAMPLE_VOICE.
"""

import argparse
import os
import sys

import pydengjen

DEFAULT_TEXT = "Hello from dengjen. This second sentence gives the modes something to split."
MODES = ("file", "lazy", "parallel", "batched", "streamed")


# tag::load[]
def load_synthesizer(manifest, speaker, parameters):
    model = pydengjen.PiperModel(manifest)
    if speaker:
        model.speaker = speaker
    if parameters:
        model.set_parameters(parameters)
    return pydengjen.Dengjen.with_piper(model)
# end::load[]


# tag::prosody[]
def output_config(args):
    return pydengjen.AudioOutputConfig(
        rate=args.rate,
        volume=args.volume,
        pitch=args.pitch,
        appended_silence_ms=args.silence,
    )
# end::prosody[]


# tag::modes[]
def synthesize_chunks(synth, mode, text, config, args):
    if mode == "lazy":
        return synth.synthesize_lazy(text, config)
    if mode == "parallel":
        return synth.synthesize_parallel(text, config)
    if mode == "batched":
        return synth.synthesize_batched(text, config, args.batch_size)
    return synth.synthesize_streamed(text, config, args.chunk_size, args.chunk_padding)
# end::modes[]


def report_chunks(chunks):
    for index, chunk in enumerate(chunks):
        if isinstance(chunk, bytes):
            print(f"chunk {index}: {len(chunk)} bytes of PCM")
        else:
            print(f"chunk {index}: {chunk.duration_ms:.0f} ms, real-time factor {chunk.real_time_factor}")


# tag::speakers[]
def print_speakers(synth):
    for speaker_id, name in sorted((synth.speakers or {}).items()):
        print(f"{speaker_id}: {name}")
# end::speakers[]


def run(args):
    synth = load_synthesizer(args.manifest, args.speaker, dict(args.param or []))
    if args.list_speakers:
        return print_speakers(synth)
    config = output_config(args)
    if args.mode == "file":
        synth.synthesize_to_file(args.out, args.text, config)
        return print(f"wrote {args.out} ({os.path.getsize(args.out)} bytes)")
    report_chunks(synthesize_chunks(synth, args.mode, args.text, config, args))


def key_value(text):
    key, sep, value = text.partition("=")
    if not key or not sep:
        raise argparse.ArgumentTypeError(f"expected KEY=VALUE, got {text!r}")
    try:
        return key, float(value)
    except ValueError:
        raise argparse.ArgumentTypeError(f"value for {key!r} must be a number, got {value!r}") from None


def add_arguments(parser):
    parser.add_argument("manifest", nargs="?", default=os.environ.get("DENGJEN_EXAMPLE_VOICE"))
    parser.add_argument("--mode", choices=MODES, default="file")
    parser.add_argument("--text", default=DEFAULT_TEXT)
    parser.add_argument("--out", default="output.wav")
    parser.add_argument("--speaker")
    parser.add_argument("--list-speakers", action="store_true")
    parser.add_argument("--param", action="append", type=key_value, metavar="KEY=VALUE")
    parser.add_argument("--rate", type=int)
    parser.add_argument("--pitch", type=int)
    parser.add_argument("--volume", type=int)
    parser.add_argument("--silence", type=int)
    parser.add_argument("--batch-size", type=int)
    parser.add_argument("--chunk-size", type=int)
    parser.add_argument("--chunk-padding", type=int)


def parse_args(argv):
    parser = argparse.ArgumentParser(description="Synthesize speech with pydengjen.")
    add_arguments(parser)
    args = parser.parse_args(argv)
    if not args.manifest:
        parser.error("pass a voice manifest or set DENGJEN_EXAMPLE_VOICE")
    return args


def main(argv=None):
    args = parse_args(argv)
    try:
        run(args)
    except pydengjen.DengjenException as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
