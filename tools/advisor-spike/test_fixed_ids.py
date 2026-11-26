"""Synthetic tests for the fixed-ID diagnostic, with no model calls."""
import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch

import test_evaluate
from fixed_ids import build_request, validate_answers, validate_labels, grade, validate_case, run


class FixedIdTests(unittest.TestCase):
    def setUp(self):
        self.requirements = {"R01": "Build services", "R02": "SOC 2 readiness"}
        self.evidence = {"E01": {"text": "Built a service"}, "E02": {"text": "Led a team"}}
        self.answers = {
            "R01": {"status": "supported", "evidence": ["E01"], "explanation": "Service delivery is stated."},
            "R02": {"status": "not_evidenced", "evidence": [], "explanation": "No SOC 2 work stated."},
        }
        self.labels = {"R01": {"status": "supported", "acceptable": ["E01"]},
                       "R02": {"status": "not_evidenced", "acceptable": []}}

    def test_valid_response_and_grade(self):
        validate_answers(self.answers, self.requirements, self.evidence)
        result = grade(self.answers, self.labels)
        self.assertEqual(result["evidence_decisions_correct"], 2)
        self.assertEqual(result["citation_sets_correct"], 2)
        self.assertEqual(result["out_of_set_citations"], 0)

    def test_missing_extra_unknown_duplicate_and_status_rules(self):
        mutations = [lambda a: a.pop("R02"), lambda a: a.update(R03=a["R01"]),
                     lambda a: a["R01"].update(evidence=["unknown"]),
                     lambda a: a["R01"].update(evidence=["E01", "E01"]),
                     lambda a: a["R01"].update(evidence=[]),
                     lambda a: a["R02"].update(evidence=["E01"]),
                     lambda a: a["R01"].update(status="maybe"),
                     lambda a: a["R01"].update(explanation="x" * 301)]
        for mutate in mutations:
            answer = copy.deepcopy(self.answers)
            mutate(answer)
            with self.subTest(answer=answer), self.assertRaises(ValueError):
                validate_answers(answer, self.requirements, self.evidence)

    def test_known_but_wrong_evidence_is_scored_not_rejected(self):
        self.answers["R01"]["evidence"] = ["E02"]
        validate_answers(self.answers, self.requirements, self.evidence)
        result = grade(self.answers, self.labels)
        self.assertEqual(result["out_of_set_citations"], 1)
        self.assertEqual(result["citation_sets_correct"], 1)

    def test_partial_is_not_full_support(self):
        self.answers["R01"]["status"] = "partial"
        result = grade(self.answers, self.labels)
        self.assertEqual(result["evidence_decisions_correct"], 2)
        self.assertEqual(result["exact_status_correct"], 1)

    def test_request_contains_only_requirements_and_evidence_not_labels(self):
        import json
        request = build_request(self.requirements, self.evidence, "local")
        prompt = json.loads(request["messages"][1]["content"])
        self.assertEqual(prompt, {"requirements": self.requirements, "evidence": self.evidence})
        self.assertEqual(request["format"]["required"], ["R01", "R02"])
        self.assertEqual(request["format"]["properties"]["R01"]["properties"]["evidence"]["items"]["enum"], ["E01", "E02"])

    def test_labels_must_match_case_and_known_evidence(self):
        validate_labels(self.labels, self.requirements, self.evidence)
        self.labels["R01"]["acceptable"] = ["unknown"]
        with self.assertRaises(ValueError):
            validate_labels(self.labels, self.requirements, self.evidence)

    def test_case_excerpt_and_variant_validation(self):
        items = [{"group": "service", "variant": "default", "text": "Built a service", "context": "Role", "evidence": ["a-0003"]}]
        case = {"requirements": {"R01": "Build services"},
                "evidence": {"E01": {"group": "service", "variant": "default"}}}
        evidence = validate_case(case, "Build services in the cloud.", items)
        self.assertEqual(evidence["E01"], items[0])
        case["requirements"]["R01"] = "Invented requirement"
        with self.assertRaises(ValueError):
            validate_case(case, "Build services in the cloud.", items)


class FixedRunTests(unittest.TestCase):
    def setUp(self):
        fixture = test_evaluate.RunTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.fixture = fixture
        self.case = fixture.root / "case.json"
        self.labels = fixture.root / "labels.json"
        self.case.write_text(json.dumps({"requirements": {"R01": "Build a service."},
                                         "evidence": {"E01": {"group": "delivery", "variant": "default"}}}))
        self.labels.write_text(json.dumps({"R01": {"status": "supported", "acceptable": ["E01"]}}))
        self.answer = {"R01": {"status": "supported", "evidence": ["E01"], "explanation": "Service delivery is stated."}}

    def transport(self, path, payload=None):
        if path != "/api/chat":
            return self.fixture.transport(path, payload)
        self.assertEqual(set(json.loads(payload["messages"][1]["content"])), {"requirements", "evidence"})
        self.assertTrue((self.fixture.output / "expected.json").exists())
        self.assertTrue((self.fixture.output / "provenance.json").exists())
        return json.dumps({"model": "local", "done": True, "done_reason": "stop",
                           "message": {"content": json.dumps(self.answer)}}).encode()

    def invoke(self):
        f = self.fixture
        with patch("evaluate.request", side_effect=self.transport), patch("fixed_ids.request", side_effect=self.transport):
            return run(f.candidate, f.jd, self.case, self.labels, f.output, "local")

    def test_run_freezes_labels_and_records_integrity(self):
        result = self.invoke()
        self.assertEqual(result["status"], "structure-valid")
        self.assertEqual(result["grade"]["citation_sets_correct"], 1)
        provenance = json.loads((self.fixture.output / "provenance.json").read_text())
        self.assertIn(str(self.labels), provenance["input_sha256"])
        self.assertFalse(provenance["expected_labels_in_request"])

    def test_rejected_answer_preserves_raw(self):
        self.answer["R01"]["evidence"] = ["invented"]
        result = self.invoke()
        self.assertEqual(result["status"], "rejected")
        self.assertTrue((self.fixture.output / "raw-response.json").exists())
        self.assertFalse((self.fixture.output / "answers.json").exists())

    def test_changing_labels_during_run_invalidates_grade(self):
        original = self.transport
        def changing_transport(path, payload=None):
            response = original(path, payload)
            if path == "/api/chat":
                self.labels.write_text("{}")
            return response
        f = self.fixture
        with patch("evaluate.request", side_effect=changing_transport), patch("fixed_ids.request", side_effect=changing_transport):
            result = run(f.candidate, f.jd, self.case, self.labels, f.output, "local")
        self.assertEqual(result["status"], "invalidated")


if __name__ == "__main__":
    unittest.main()
