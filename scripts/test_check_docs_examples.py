import tempfile
import unittest
from pathlib import Path

from check_docs_examples import check

PAGE_OK = "include::example$python/demo.py[tag=body]\n"


def build_repo(root, page=PAGE_OK, demo="# tag::body[]\nprint(1)\n# end::body[]\n"):
    (root / "examples" / "python").mkdir(parents=True)
    (root / "examples" / "python" / "demo.py").write_text(demo)
    pages = root / "docs" / "modules" / "ROOT" / "pages"
    pages.mkdir(parents=True)
    (pages / "start.adoc").write_text(page)
    link = root / "docs" / "modules" / "ROOT" / "examples"
    link.mkdir()
    (link / "python").symlink_to("../../../../examples/python")
    return root


class CheckTests(unittest.TestCase):
    def check(self, **kwargs):
        with tempfile.TemporaryDirectory() as tmp:
            return check(build_repo(Path(tmp), **kwargs))

    def test_clean_repo_has_no_problems(self):
        self.assertEqual(self.check(), [])

    def test_syntax_error_in_python_example_is_reported(self):
        problems = self.check(demo="# tag::body[]\ndef (:\n# end::body[]\n")
        self.assertTrue(any("demo.py" in p for p in problems))

    def test_missing_include_target_is_reported(self):
        problems = self.check(page="include::example$python/absent.py[]\n")
        self.assertTrue(any("absent.py" in p for p in problems))

    def test_missing_tag_is_reported(self):
        problems = self.check(page="include::example$python/demo.py[tag=nope]\n")
        self.assertTrue(any("nope" in p for p in problems))

    def test_unclosed_tag_is_reported(self):
        problems = self.check(demo="# tag::body[]\nprint(1)\n")
        self.assertTrue(any("body" in p for p in problems))

    def test_invalid_json_block_is_reported(self):
        page = "[source,json]\n----\n{\"a\": }\n----\n"
        problems = self.check(page=page)
        self.assertTrue(any("json" in p.lower() for p in problems))

    def test_valid_json_block_passes(self):
        page = "[source,json]\n----\n{\"a\": 1}\n----\n"
        self.assertEqual(self.check(page=page), [])

    def test_shell_syntax_error_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = build_repo(Path(tmp))
            (root / "examples" / "python" / "bad.sh").write_text("if then\n")
            problems = check(root)
        self.assertTrue(any("bad.sh" in p for p in problems))

    def test_real_directory_in_docs_examples_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = build_repo(Path(tmp))
            (root / "docs" / "modules" / "ROOT" / "examples" / "copy").mkdir()
            problems = check(root)
        self.assertTrue(any("copy" in p for p in problems))

    def test_symlink_outside_examples_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = build_repo(Path(tmp))
            (root / "elsewhere").mkdir()
            (root / "docs" / "modules" / "ROOT" / "examples" / "out").symlink_to("../../../../elsewhere")
            problems = check(root)
        self.assertTrue(any("out" in p for p in problems))

    def test_multi_tag_include_reports_only_the_missing_tag(self):
        problems = self.check(page="include::example$python/demo.py[tags=body;nope]\n")
        self.assertTrue(any("nope" in p for p in problems))
        self.assertFalse(any("tag::body" in p for p in problems))

    def test_json_block_with_extra_attributes_is_validated(self):
        page = "[source,json,subs=attributes]\n----\n{\"a\": }\n----\n"
        problems = self.check(page=page)
        self.assertTrue(any("json" in p.lower() for p in problems))


if __name__ == "__main__":
    unittest.main()
