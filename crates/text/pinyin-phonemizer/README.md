# Pinyin phonemizer

Mandarin pinyin grapheme-to-phoneme conversion for the dengjen-tts speech synthesis engine.

Resolves each character's pinyin via dictionary lookup, routing configured polyphonic characters through a G2PW ONNX model to disambiguate them, then maps bopomofo output to pinyin.

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
