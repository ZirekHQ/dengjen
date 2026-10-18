import os
import threading
import unittest

import pydengjen

MODEL_DIR = os.environ.get("DENGJEN_STT_MODEL_DIR")
MODEL_SHA = os.environ.get("DENGJEN_STT_MODEL_SHA256")
HAS_MODEL = bool(MODEL_DIR and MODEL_SHA)
HAS_STT = hasattr(pydengjen, "Recognizer")


@unittest.skipUnless(
    os.environ.get("DENGJEN_REQUIRE_STT") == "1", "set DENGJEN_REQUIRE_STT=1 on builds with the stt feature"
)
class SttFeaturePresenceTest(unittest.TestCase):
    def test_the_module_exposes_the_recognizer_and_its_error(self):
        self.assertTrue(HAS_STT)
        self.assertTrue(hasattr(pydengjen, "SttError"))


@unittest.skipUnless(HAS_STT, "built without the stt feature")
class RecognizerTest(unittest.TestCase):
    def test_unknown_model_dir_is_an_unreadable_model_error(self):
        with self.assertRaises(pydengjen.SttError) as ctx:
            pydengjen.Recognizer("/definitely/not/here", "0" * 64)
        self.assertEqual(ctx.exception.code, 41)

    def test_an_stt_error_is_a_dengjen_exception(self):
        self.assertTrue(issubclass(pydengjen.SttError, pydengjen.DengjenException))

    def test_zero_threads_is_an_invalid_configuration(self):
        with self.assertRaises(pydengjen.SttError) as ctx:
            pydengjen.Recognizer("/definitely/not/here", "0" * 64, 0)
        self.assertEqual(ctx.exception.code, 24)

    @unittest.skipUnless(HAS_MODEL, "needs DENGJEN_STT_MODEL_DIR and DENGJEN_STT_MODEL_SHA256")
    def test_close_is_idempotent_and_recognize_after_close_fails(self):
        recognizer = pydengjen.Recognizer(MODEL_DIR, MODEL_SHA)
        recognizer.close()
        recognizer.close()
        with self.assertRaises(pydengjen.SttError) as ctx:
            recognizer.recognize([0.0] * 1600, 16000)
        self.assertEqual(ctx.exception.code, -1000)

    @unittest.skipUnless(HAS_MODEL, "needs a model")
    def test_empty_and_zero_rate_audio_are_invalid(self):
        with pydengjen.Recognizer(MODEL_DIR, MODEL_SHA) as recognizer:
            for samples, rate in (([], 16000), ([0.0] * 1600, 0)):
                with self.assertRaises(pydengjen.SttError) as ctx:
                    recognizer.recognize(samples, rate)
                self.assertEqual(ctx.exception.code, 44)

    @unittest.skipUnless(HAS_MODEL, "needs a model")
    def test_silence_recognizes_without_error(self):
        with pydengjen.Recognizer(MODEL_DIR, MODEL_SHA) as recognizer:
            self.assertIsInstance(recognizer.recognize([0.0] * 16000, 16000), str)

    @unittest.skipUnless(HAS_MODEL, "needs a model")
    def test_threads_recognizing_and_closing_together_finish(self):
        recognizer = pydengjen.Recognizer(MODEL_DIR, MODEL_SHA)
        outcomes = []

        def work():
            try:
                outcomes.append(recognizer.recognize([0.0] * 48000, 16000))
            except pydengjen.SttError as error:
                outcomes.append(error.code)

        threads = [threading.Thread(target=work, daemon=True) for _ in range(4)]
        for thread in threads:
            thread.start()
        recognizer.close()
        for thread in threads:
            thread.join(timeout=120)
        self.assertFalse([t for t in threads if t.is_alive()], "a recognizing thread is stuck")
        self.assertEqual(len(outcomes), 4)
