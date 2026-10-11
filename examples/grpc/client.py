"""Calls dengjen-tts-grpc with grpcio.

Usage: python client.py [VOICE_MANIFEST_JSON] [--address HOST:PORT] [--out output.wav] [--text TEXT]
The manifest and address default to DENGJEN_EXAMPLE_VOICE and DENGJEN_GRPC_ADDRESS (127.0.0.1:49314).

The server streams raw 16-bit PCM (no WAV header); write_pcm wraps it using the voice's AudioFormat.

Generate the stubs first, next to this file:
    python -m grpc_tools.protoc -I. --python_out=. --grpc_python_out=. dengjen_grpc.proto
"""

import argparse
import os
import sys
import wave

import grpc

# tag::stubs[]
import dengjen_grpc_pb2 as pb
import dengjen_grpc_pb2_grpc as rpc
# end::stubs[]


# tag::load[]
def load_voice(stub, manifest):
    return stub.LoadVoice(pb.VoiceConfigLocation(path=manifest))
# end::load[]


# tag::synthesize[]
def synthesize(stub, voice_key, text):
    request = pb.SynthesisRequest(
        voice_key=voice_key, text=text, synthesis_mode=pb.MODE_LAZY
    )
    return [chunk.audio_bytes for chunk in stub.SynthesizeUtterance(request)]
# end::synthesize[]


# tag::realtime[]
def synthesize_realtime(stub, voice_key, text):
    request = pb.SynthesisRequest(voice_key=voice_key, text=text)
    return b"".join(chunk.audio_bytes for chunk in stub.SynthesizeUtteranceRealtime(request))
# end::realtime[]


def write_pcm(path, pcm, audio):
    with wave.open(path, "wb") as out:
        out.setnchannels(audio.num_channels)
        out.setsampwidth(audio.sample_width)
        out.setframerate(audio.sample_rate)
        out.writeframes(pcm)


def run(args):
    with grpc.insecure_channel(args.address) as channel:
        stub = rpc.DengjenGrpcStub(channel)
        print("server version:", stub.GetDengjenVersion(pb.Empty()).version)
        voice = load_voice(stub, args.manifest)
        print("voice_key:", voice.voice_key)
        chunks = synthesize(stub, voice.voice_key, args.text)
        print(f"received {len(chunks)} PCM chunk(s)")
        write_pcm(args.out, b"".join(chunks), voice.audio)
        print(f"wrote {args.out}")
        if voice.supports_streaming_output:
            pcm = synthesize_realtime(stub, voice.voice_key, args.text)
            realtime_out = f"{os.path.splitext(args.out)[0]}.realtime.wav"
            write_pcm(realtime_out, pcm, voice.audio)
            print(f"wrote {realtime_out}")


def parse_args(argv):
    parser = argparse.ArgumentParser(description="Call dengjen-tts-grpc.")
    parser.add_argument("manifest", nargs="?", default=os.environ.get("DENGJEN_EXAMPLE_VOICE"))
    parser.add_argument("--address", default=os.environ.get("DENGJEN_GRPC_ADDRESS", "127.0.0.1:49314"))
    parser.add_argument("--text", default="Hello from the gRPC server.")
    parser.add_argument("--out", default="output.wav")
    args = parser.parse_args(argv)
    if not args.manifest:
        parser.error("pass a voice manifest or set DENGJEN_EXAMPLE_VOICE")
    return args


def main(argv=None):
    try:
        run(parse_args(argv))
    except grpc.RpcError as error:
        print(f"error: {error.code().name}: {error.details()}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
