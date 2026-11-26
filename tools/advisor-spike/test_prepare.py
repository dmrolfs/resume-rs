"""Synthetic tests only; never load the owner's private sources."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from prepare import prepare


class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "original.md"
        self.source.write_text("# Ada\n\n- Built a service.\n- Led a team.\n")
        self.recipe = {
            "sources": {"a": {"path": str(self.source), "sha256": hashlib.sha256(self.source.read_bytes()).hexdigest()}},
            "nodes": [
                {"sources": [["a", 1]], "group": None, "variant": None, "tags": [], "verify": False},
                {"sources": [["a", 3], ["a", 4]], "group": "delivery", "variant": "default", "tags": ["platform"], "verify": True},
            ],
            "omissions": [],
            "decisions": [{"id": "D1", "status": "pending", "note": "Review merged wording."}],
        }

    def run_prepare(self):
        recipe_path = self.root / "recipe.json"
        recipe_path.write_text(json.dumps(self.recipe))
        return prepare(recipe_path, self.root / "candidate")

    def test_keeps_sources_and_maps_every_nonblank_line(self):
        original = self.source.read_bytes()
        self.run_prepare()
        self.assertEqual(self.source.read_bytes(), original)
        mapping = json.loads((self.root / "candidate/source-map.json").read_text())
        self.assertEqual([r["line"] for r in mapping], [1, 3, 4])
        baseline = (self.root / "candidate/Resume-Baseline.md").read_text()
        self.assertIn("- Built a service. Led a team. <!-- =delivery.default #platform @verify -->", baseline)
        self.assertEqual(mapping[1]["baseline_line"], mapping[2]["baseline_line"])
        self.assertEqual(mapping[1]["source_text"], "- Built a service.")

    def test_source_hash_mismatch_refused(self):
        self.source.write_text("changed")
        with self.assertRaisesRegex(ValueError, "hash"):
            self.run_prepare()
        self.assertFalse((self.root / "candidate").exists())

    def test_missing_or_duplicate_source_coverage_refused(self):
        for sources in ([["a", 3]], [["a", 3], ["a", 3], ["a", 4]]):
            with self.subTest(sources=sources):
                self.recipe["nodes"][1]["sources"] = sources
                with self.assertRaisesRegex(ValueError, "coverage"):
                    self.run_prepare()

    def test_existing_destination_never_overwritten(self):
        self.run_prepare()
        with self.assertRaises(FileExistsError):
            self.run_prepare()

    def test_explicit_omission_is_preserved_in_mapping(self):
        self.recipe["nodes"][1]["sources"] = [["a", 3]]
        self.recipe["omissions"] = [{"source": ["a", 4], "reason": "Duplicate detail, owner review pending."}]
        self.run_prepare()
        rows = json.loads((self.root / "candidate/source-map.json").read_text())
        self.assertEqual(rows[-1]["disposition"], "omitted-proposed")
        self.assertEqual(rows[-1]["source_text"], "- Led a team.")

    def test_invalid_group_without_default_refused(self):
        self.recipe["nodes"][1]["variant"] = "org"
        with self.assertRaisesRegex(ValueError, "default"):
            self.run_prepare()

    def test_nonadjacent_group_refused(self):
        first = {**self.recipe["nodes"][1], "sources": [["a", 3]]}
        self.recipe["nodes"] = [first, self.recipe["nodes"][0],
                                {**first, "sources": [["a", 4]], "variant": "org"}]
        with self.assertRaisesRegex(ValueError, "Nonadjacent"):
            self.run_prepare()

    def test_identical_shared_header_is_deduplicated(self):
        self.recipe["sources"]["b"] = self.recipe["sources"]["a"].copy()
        for node in self.recipe["nodes"]:
            node["sources"] += [["b", line] for _, line in list(node["sources"])]
        self.run_prepare()
        text = (self.root / "candidate/Resume-Baseline.md").read_text()
        self.assertEqual(text.count("# Ada"), 1)
        self.assertEqual(text.count("Built a service."), 1)

    def test_output_files_are_owner_only(self):
        self.run_prepare()
        output = self.root / "candidate"
        self.assertEqual(output.stat().st_mode & 0o777, 0o700)
        for path in output.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_mixed_bullet_and_heading_merge_refused(self):
        self.recipe["nodes"][1]["sources"].append(["a", 1])
        self.recipe["nodes"].pop(0)
        with self.assertRaisesRegex(ValueError, "whole bullet"):
            self.run_prepare()

    def test_no_writing_inside_code_repository(self):
        with self.assertRaisesRegex(ValueError, "repository"):
            prepare(Path("missing.json"), Path(__file__).resolve().parent / "private")


if __name__ == "__main__":
    unittest.main()
