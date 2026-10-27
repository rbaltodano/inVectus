"""Model-free checks for evaluation leakage, metrics and offline installation guards."""

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from artifacts import download, files, verify
from evaluate import rank_metrics, unit


class EvaluationTests(unittest.TestCase):
    def test_duplicate_targets_count_as_two_relevant_results(self):
        m = rank_metrics([[1, 0]], [[1, 0], [0, 1], [1, 0]], ["a"], ["a", "b", "a"])
        self.assertEqual(m["hit_at_1"], 1)
        self.assertEqual(m["recall_at_1"], 0.5)
        self.assertEqual(m["recall_at_5"], 1)
        self.assertEqual(m["reciprocal_rank"], 1)

    def test_failure_and_invalid_vectors_are_visible(self):
        m = rank_metrics([[1, 0]], [[0, 1], [1, 0]], ["a"], ["a", "b"])
        self.assertEqual(m["hit_at_1"], 0)
        self.assertEqual(m["reciprocal_rank"], 0.5)
        with self.assertRaises(ValueError):
            rank_metrics([[1, 0]], [[1, 0]], ["missing"], ["a"])
        for v in [[[float("nan"), 1]], [[0, 0]]]:
            with self.assertRaises(ValueError):
                unit(v)

    def test_download_refuses_offline_without_opening_network(self):
        with (
            patch.dict(os.environ, {"INVECTUS_OFFLINE": "1"}),
            patch("urllib.request.urlopen") as network,
        ):
            with self.assertRaises(ValueError):
                download("gemma", Path("/does-not-exist"))
            network.assert_not_called()

    def test_installer_rejects_tampered_local_artifact(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            first = next(files("gemma"))
            path = root / first["rfilename"]
            path.parent.mkdir(parents=True)
            path.write_bytes(b"x" * first["size"])
            with self.assertRaises(ValueError):
                verify("gemma", root)

    def test_manifest_groups_are_disjoint_and_explicitly_unreviewed(self):
        data = json.loads(
            (
                Path(__file__).resolve().parents[2] / "docs/multimodal/fixtures-v1.json"
            ).read_text()
        )
        self.assertIn("not human adjudicated", data["judgments"]["status"])
        ids = [x["id"] for x in data["assets"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual({x["split"] for x in data["assets"]}, {"dev", "holdout"})
        for x in data["assets"]:
            for key in ["document", "near_duplicate", "image_query"]:
                self.assertEqual(len(x[key]["sha256"]), 64)
            self.assertIn(x["license"], ["CC0-1.0", "public-domain"])


if __name__ == "__main__":
    unittest.main()
