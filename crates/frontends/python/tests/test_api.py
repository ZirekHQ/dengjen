import inspect
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import pydengjen

DATA_ENV = "DENGJEN_ESPEAKNG_DATA_DIRECTORY"
PACKAGE_DIR = Path(pydengjen.__file__).resolve().parent
PROBE = (
    "import ctypes, pydengjen\n"
    "libc = ctypes.CDLL(None)\n"
    "libc.getenv.restype = ctypes.c_char_p\n"
    f"print((libc.getenv({DATA_ENV.encode()!r}) or b'').decode())\n"
)


FIXTURES = Path(__file__).resolve().parents[3] / "dengjen/models/piper/tests/fixtures"
BATCH_MODEL = FIXTURES / "synthetic_piper_batch.onnx"
PHONEME_ID_MAP = {"^": [1], "$": [2], "_": [3], "t": [4], "\u025b": [5], "s": [6]}


def write_voice(directory, **overrides):
    shutil.copy(BATCH_MODEL, directory / BATCH_MODEL.name)
    config = {
        "key": None,
        "language": {"code": "en-US"},
        "audio": {"sample_rate": 22050, "quality": None},
        "num_speakers": 1,
        "speaker_id_map": {"default": 0},
        "streaming": False,
        "espeak": {"voice": "en-us"},
        "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8},
        "num_symbols": 8,
        "phoneme_map": {},
        "phoneme_id_map": PHONEME_ID_MAP,
        "phoneme_type": "text",
        "hop_length": 256,
    }
    config.update(overrides)
    config_path = directory / "voice.onnx.json"
    config_path.write_text(json.dumps(config))
    return config_path


def defaults(callable_):
    parameters = inspect.signature(callable_).parameters.values()
    return {p.name: p.default for p in parameters if p.default is not inspect.Parameter.empty}


def data_directory_seen_by_a_fresh_interpreter(value):
    env = {k: v for k, v in os.environ.items() if k != DATA_ENV}
    if value is not None:
        env[DATA_ENV] = value
    result = subprocess.run(
        [sys.executable, "-c", PROBE], env=env, capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


class OptionalArgumentTests(unittest.TestCase):
    def test_audio_output_config_needs_no_arguments(self):
        pydengjen.AudioOutputConfig()
        pydengjen.AudioOutputConfig(rate=60)

    def test_synthesis_methods_default_their_optional_arguments_to_none(self):
        expected = {
            "synthesize": ["audio_output_config"],
            "synthesize_lazy": ["audio_output_config"],
            "synthesize_parallel": ["audio_output_config"],
            "synthesize_batched": ["audio_output_config", "batch_size"],
            "synthesize_streamed": ["audio_output_config", "chunk_size", "chunk_padding"],
            "synthesize_to_file": ["audio_output_config"],
        }
        for name, optional in expected.items():
            with self.subTest(method=name):
                found = defaults(getattr(pydengjen.Dengjen, name))
                self.assertEqual(found, {key: None for key in optional})

    def test_phonemize_text_defaults_everything_after_the_language_to_none(self):
        names = ["phoneme_separator", "remove_lang_switch_flags", "remove_stress", "use_tashkeel"]
        self.assertEqual(defaults(pydengjen.phonemize_text), {name: None for name in names})


class PiperScalesTests(unittest.TestCase):
    def test_values_are_readable(self):
        scales = pydengjen.PiperScales(1.0, 0.5, 0.25)
        self.assertEqual((scales.length_scale, scales.noise_scale, scales.noise_w), (1.0, 0.5, 0.25))


class ExceptionHierarchyTests(unittest.TestCase):
    def test_specific_errors_keep_the_base_exception(self):
        for name in (
            "ResourceLoadError",
            "PhonemizationError",
            "InferenceError",
            "InvalidConfigurationError",
            "UnsupportedOperationError",
        ):
            with self.subTest(error=name):
                self.assertTrue(issubclass(getattr(pydengjen, name), pydengjen.DengjenException))


class VoiceTests(unittest.TestCase):
    def setUp(self):
        self._directory = tempfile.TemporaryDirectory()
        self.addCleanup(self._directory.cleanup)
        self.directory = Path(self._directory.name)

    def test_a_missing_config_raises_a_dengjen_exception(self):
        with self.assertRaises(pydengjen.DengjenException):
            pydengjen.PiperModel(str(self.directory / "missing.json"))

    @unittest.skipUnless(BATCH_MODEL.is_file(), "synthetic Piper fixture not available")
    def test_an_unknown_model_type_is_rejected_instead_of_loading_as_piper(self):
        config_path = write_voice(self.directory, model_type="bark")
        with self.assertRaises(pydengjen.InvalidConfigurationError) as raised:
            pydengjen.PiperModel(str(config_path))
        self.assertIn("bark", str(raised.exception))

    @unittest.skipUnless(BATCH_MODEL.is_file(), "synthetic Piper fixture not available")
    def test_each_stream_yields_audio_until_exhausted(self):
        model = pydengjen.PiperModel(str(write_voice(self.directory)))
        engine = pydengjen.Dengjen.with_piper(model)
        streams = {
            "lazy": engine.synthesize_lazy("Test one. Test two."),
            "parallel": engine.synthesize_parallel("Test one. Test two."),
            "batched": engine.synthesize_batched("Test one. Test two.", batch_size=1),
        }
        for name, stream in streams.items():
            with self.subTest(stream=name):
                chunks = list(stream)
                self.assertGreater(len(chunks), 0)
                self.assertEqual(chunks[0].sample_rate, 22050)
                self.assertGreater(len(chunks[0].get_wave_bytes()), 44)
                with self.assertRaises(StopIteration):
                    next(stream)


@unittest.skipIf(sys.platform == "win32", "the probe reads the C environment through libc")
class EspeakDataTests(unittest.TestCase):
    def test_the_package_ships_the_data(self):
        self.assertTrue((PACKAGE_DIR / "espeak-ng-data" / "phontab").is_file())

    def test_directory_defaults_to_the_package(self):
        self.assertEqual(data_directory_seen_by_a_fresh_interpreter(None), str(PACKAGE_DIR))

    def test_a_directory_the_user_set_is_kept(self):
        self.assertEqual(data_directory_seen_by_a_fresh_interpreter("/somewhere/else"), "/somewhere/else")


if __name__ == "__main__":
    unittest.main()
