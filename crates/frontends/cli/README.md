# dengjen-tts-cli

Command-line interface for the dengjen-tts speech synthesis engine.

Builds the `dengjen` binary. `dengjen speak <config|voice-key>` reads text from a file or stdin, synthesizes it with a chosen model config in Lazy, Parallel, Batched, or Realtime mode, and writes the resulting audio to a file or stdout.

`dengjen voices list|install|remove|refresh` manages Piper voices; see the usage guide. `dengjen <config>` was replaced by `dengjen speak <config>`.

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
