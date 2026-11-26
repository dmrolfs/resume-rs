"""Synthetic segment, coverage-review and approval-gate regressions."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import advise
import requirements as req
from llm_client import Config, Client
from test_advisor import BASELINE, JD, ASSESSMENT, envelope, approval_for


class RequirementsTests(unittest.TestCase):
    def test_segments_exact_offsets_unicode_and_whole_long_sentence(self):
        jd = '  Leadership.\r\n\r\n' + 'Über ' * 100 + 'ends. Another sentence.'
        source = req.segments(jd)
        self.assertEqual([s['id'] for s in source], [1, 2, 3])
        for s in source:
            self.assertEqual(jd[s['start']:s['end']], s['text'])
        self.assertEqual(source[-1]['text'], 'Another sentence.')
        long = req.segments('word ' * 500)
        self.assertEqual(len(long), 1)
        self.assertEqual(len(req.batches(long)), 1)  # never silently truncate

    def test_batches_preserve_all_source_ids(self):
        source = req.segments('\n'.join('x' * 900 for _ in range(5)))
        batched = req.batches(source)
        self.assertEqual([len(b) for b in batched], [1, 1, 1, 1, 1])
        self.assertEqual([s for b in batched for s in b], source)

    def test_batch_validation_and_ambiguous_quotes(self):
        source = req.segments('Manage budgets and hiring.\nReview budgets.')
        good = {'clauses': [{'segment': 1, 'quote': 'budgets', 'importance': 'required'}]}
        req.validate_batch(good, source[:1])
        for changes in [{'segment': 2}, {'segment': True}, {'quote': ''}, {'quote': 'invented'}, {'importance': 'maybe'}]:
            bad = {'clauses': [{**good['clauses'][0], **changes}]}
            with self.assertRaises(ValueError):
                req.validate_batch(bad, source[:1])
        with self.assertRaises(ValueError):
            req.validate_batch({'clauses': good['clauses'] * 40}, source)
        with self.assertRaises(ValueError):
            req.validate_batch(good, req.segments('budgets and budgets'))
        req.validate_batch({'clauses': []}, source)

    def test_materialized_ids_parents_and_review_omissions(self):
        source = req.segments('Manage budgets.\nExperience with budgets.\nBenefits only.')
        rows = [{'segment': 2, 'quote': 'budgets', 'importance': 'preferred'},
                {'segment': 1, 'quote': 'budgets', 'importance': 'required'}]
        req.validate_batch({'clauses': rows}, source)
        result = req.materialize(rows, source)
        self.assertEqual(result['clauses'][0]['id'], 'r001')
        self.assertEqual(result['clauses'][0]['parent'], 'Manage budgets.')
        advise.validate_clauses(result, '\n'.join(s['text'] for s in source))
        review = req.review_text(source, result)
        self.assertIn('NO EXTRACTED CLAUSES', review)
        self.assertIn('[preferred] budgets', review)

    def test_repeated_occurrences_and_overlapping_quotes(self):
        jd = 'Required:\nManage budgets.\nPreferred:\nManage budgets.'
        source = req.segments(jd)
        row = {'segment': 2, 'quote': 'budgets', 'importance': 'required'}
        first = req.materialize([row], source)
        self.assertEqual(req.review_text(source, first).count('r001 [required]'), 1)
        both = req.materialize([row, {**row, 'segment': 4, 'importance': 'preferred'}], source)
        advise.validate_clauses(both, jd)
        self.assertNotEqual(*[r['source_start'] for r in both['clauses']])
        with self.assertRaises(ValueError):
            req.validate_batch({'clauses': [{'segment': 1, 'quote': 'aa', 'importance': 'required'}]}, req.segments('aaa'))
        with self.assertRaises(ValueError):
            advise.validate_clauses({'clauses': [{'id': 'r01', 'parent': 'aaa', 'quote': 'aa', 'importance': 'required'}]}, 'aaa')

    def test_missing_or_stale_approval_prevents_network_and_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            jd, baseline, clauses = [root / n for n in ['jd', 'baseline', 'clauses']]
            jd.write_text(JD)
            baseline.write_text(BASELINE)
            clauses.write_text(json.dumps({'clauses': [{'id': 'r01', 'parent': JD, 'quote': JD, 'importance': 'required'}]}))
            for approval in [None, {**approval_for(jd, clauses), 'clauses_sha256': 'stale'}]:
                with patch.object(Client, 'send') as send:
                    with self.assertRaises(ValueError):
                        advise.run(Config('http://127.0.0.1/v1', 'm'), baseline, jd, root / 'out', clauses, approval=approval)
                    send.assert_not_called()
                self.assertFalse((root / 'out').exists())

    def test_approved_assessment_skips_context_and_has_stable_schema(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            jd, baseline, clauses = [root / n for n in ['jd', 'baseline', 'clauses']]
            jd.write_text('Experience improving:\n' + JD + '\nA growing company.')
            baseline.write_text(BASELINE)
            clauses.write_text(json.dumps({'clauses': [
                {'id': 'r01', 'parent': JD, 'quote': JD, 'importance': 'required'},
                {'id': 'r02', 'parent': 'A growing company.', 'quote': 'A growing company.', 'importance': 'context'}]}))
            with patch.object(Client, 'send', return_value=envelope(ASSESSMENT)) as send:
                advise.run(Config('http://127.0.0.1/v1', 'm'), baseline, jd, root / 'out', clauses,
                           approval=approval_for(jd, clauses))
                send.assert_called_once()
                payload = send.call_args.args[0]
                self.assertTrue(payload['messages'][1]['content'].startswith('{"evidence":'))
                self.assertIn('governing verb', payload['messages'][0]['content'])
                self.assertEqual(json.loads(payload['messages'][1]['content'])['preceding_source_context'], 'Experience improving:\n')
            report = json.loads((root / 'out/report.json').read_text())
            self.assertEqual(report['skipped_context'], ['r02'])
            ev = advise.evidence_from_markdown(BASELINE)
            self.assertEqual(advise.assessment_schema('r01', ev), advise.assessment_schema('r02', ev))
            with self.assertRaises(ValueError):
                advise.validate_assessment(ASSESSMENT, 'r02', ev)


if __name__ == '__main__':
    unittest.main()
