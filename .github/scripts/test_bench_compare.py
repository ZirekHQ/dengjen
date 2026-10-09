import importlib.util
import io
import pathlib
import unittest

HERE = pathlib.Path(__file__).parent
spec = importlib.util.spec_from_file_location("bench_compare", HERE / "bench-compare.py")
bc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bc)


def data(name):
    return (HERE / "testdata" / name).read_text(encoding="utf-8")


class ParseTest(unittest.TestCase):
    def test_flat_row_median_in_ns(self):
        self.assertEqual(
            bc.parse(data("divan_audio_ops.txt")),
            {"bench_overlap_with": 362_200.0},
        )

    def test_nested_paths_and_units(self):
        parsed = bc.parse(data("divan_nested.txt"))
        self.assertEqual(parsed["speech_streams/bench_lazy_stream"], 1.4e9)
        self.assertEqual(parsed["speech_streams/bench_lazy_stream/t=4"], 2.2e9)
        self.assertEqual(parsed["speech_streams/bench_lazy_stream_latency"], 350.0e6)

    def test_group_rows_without_values_are_not_emitted(self):
        self.assertNotIn("speech_streams", bc.parse(data("divan_nested.txt")))

    def test_unknown_unit_raises(self):
        with self.assertRaises(ValueError):
            bc.to_ns("1.0 parsecs")


class CompareTest(unittest.TestCase):
    def test_two_x_is_regression_one_point_nine_is_not(self):
        rows = bc.compare({"a": 100.0, "b": 100.0}, {"a": 200.0, "b": 190.0}, 2.0)
        by_path = {r.path: r for r in rows}
        self.assertTrue(by_path["a"].regressed)
        self.assertFalse(by_path["b"].regressed)

    def test_case_missing_from_base_is_na_not_regression(self):
        (row,) = bc.compare({}, {"kokoro": 5.0}, 2.0)
        self.assertIsNone(row.base_ns)
        self.assertIsNone(row.ratio)
        self.assertFalse(row.regressed)

    def test_case_missing_from_head_is_listed_not_regression(self):
        (row,) = bc.compare({"gone": 5.0}, {}, 2.0)
        self.assertIsNone(row.head_ns)
        self.assertFalse(row.regressed)

    def test_zero_base_median_has_no_ratio(self):
        (row,) = bc.compare({"z": 0.0}, {"z": 10.0}, 2.0)
        self.assertIsNone(row.ratio)
        self.assertFalse(row.regressed)


class MainTest(unittest.TestCase):
    def run_main(self, base, head):
        out = io.StringIO()
        code = bc.main(
            ["base", "head"],
            read=lambda p: {"base": base, "head": head}[p],
            out=out,
        )
        return code, out.getvalue()

    def test_regression_warns_and_exits_zero(self):
        slow = data("divan_audio_ops.txt").replace("362.2 µs", "800.0 µs")
        code, out = self.run_main(data("divan_audio_ops.txt"), slow)
        self.assertEqual(code, 0)
        self.assertIn("::warning::", out)
        self.assertIn("bench_overlap_with", out)

    def test_zero_rows_in_head_fails(self):
        code, _ = self.run_main(data("divan_audio_ops.txt"), "Timer precision: 10 ns\n")
        self.assertEqual(code, 2)

    def test_zero_rows_in_base_fails(self):
        code, _ = self.run_main("", data("divan_audio_ops.txt"))
        self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
