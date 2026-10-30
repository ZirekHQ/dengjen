# dengjen-tts-grpc

gRPC server frontend for the dengjen-tts speech synthesis engine.

Serves the `DengjenGrpc` service defined in `proto/dengjen_grpc.proto` over `tonic`, loading voices from a config path and streaming synthesized audio back to clients.

## Limits

The server caps its resource use. Each cap reads an environment variable; unset, zero or unparsable values keep the default.

| Variable | Default | Effect when exceeded |
| --- | --- | --- |
| `DENGJEN_GRPC_MAX_VOICES` | `16` | `LoadVoice` of a new voice returns `RESOURCE_EXHAUSTED`. |
| `DENGJEN_GRPC_MAX_TEXT_BYTES` | `65536` | Synthesis requests with longer text return `INVALID_ARGUMENT`. |
| `DENGJEN_GRPC_MAX_CONCURRENT_SYNTHESIS` | `4` | Synthesis and voice loads beyond the cap return `RESOURCE_EXHAUSTED`. |
| `DENGJEN_GRPC_MAX_CONCURRENT_RECOGNITION` | `1` | `Recognize` calls beyond the cap return `RESOURCE_EXHAUSTED`. |
| `DENGJEN_GRPC_MAX_MESSAGE_BYTES` | `16777216` | Maximum gRPC message size, applied to requests and responses. |

`LoadVoice` logs the underlying filesystem or parse error server-side and returns a generic message to the client.

## Speech recognition

`Recognize` takes mono 16-bit little-endian PCM (`pcm_s16le`) and its `sample_rate` and returns the transcript. It exists only in builds with the `stt` cargo feature; other builds answer `UNIMPLEMENTED`.

| Variable | Effect |
| --- | --- |
| `DENGJEN_GRPC_STT_MODEL_DIR` | Directory holding `model.int8.onnx` and `tokens.txt`. |
| `DENGJEN_GRPC_STT_MODEL_SHA256` | Expected SHA-256 of `model.int8.onnx`. |

Set both or neither. The server verifies the checksum and loads the model at startup and exits on failure. With neither set, `Recognize` returns `FAILED_PRECONDITION`. Empty audio, audio shorter than 25 ms, an odd byte count or a sample rate that is zero or above `i32::MAX` returns `INVALID_ARGUMENT`. A failed decode returns `INTERNAL`.

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
