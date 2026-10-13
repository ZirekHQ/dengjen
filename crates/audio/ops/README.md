# dengjen-audio-ops

Audio post-processing primitives shared across dengjen-tts's speech synthesis pipeline: sample buffers, Hann windowing, WAV encoding, and `SonicStream`, a safe Rust port of the [Sonic](https://github.com/waywardgeek/sonic) speech rate, pitch, and volume library (Apache-2.0).

Exposes `Audio`/`AudioSamples`/`AudioInfo` sample types alongside functions for writing synthesized audio out as WAV, either to a buffer or to a file.
