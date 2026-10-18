# dengjen-tts-python

Python bindings to `dengjen-tts` using `pyo3`. Published to PyPI as `pydengjen`.

Install, voice download and a runnable example:
<https://zirekhq.github.io/dengjen-tts/next/start-python.html>. The example source is
[`examples/python/synthesize.py`](https://github.com/ZirekHQ/dengjen/blob/main/examples/python/synthesize.py).

## Speech recognition

Wheels for Linux x86_64, Windows x64 and macOS arm64 include an offline recognizer. It needs a
model directory (`model.int8.onnx` and `tokens.txt`) and the SHA-256 of the model file:

```python
import pydengjen

with pydengjen.Recognizer(model_dir, model_sha256) as recognizer:
    text = recognizer.recognize(samples, 16000)  # mono floats in [-1, 1]
```

Failures raise `pydengjen.SttError`, a `DengjenException` whose `.code` is the `libdengjen-stt`
error code. Other wheels, and builds without the `stt` feature, have no `Recognizer`.

