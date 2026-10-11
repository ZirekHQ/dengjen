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
| `DENGJEN_GRPC_MAX_MESSAGE_BYTES` | `16777216` | Maximum gRPC message size, applied to requests and responses. |

`LoadVoice` logs the underlying filesystem or parse error server-side and returns a generic message to the client.
