"""Synthetic tests for fixture freezing and frontier-reference comparison; no gateway traffic."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import advise
import model_compare as mc

JD = 'Manage engineering budgets. Coach engineers.'
BASELINE = '# Example\n\n## Summary\n\nManaged a $4M budget. <!-- =scope.default #org -->\n'
CLAUSES = {'clauses': [
    {'id': 'r01', 'quote': 'Manage engineering budgets.', 'parent': 'Manage engineering budgets.', 'importance': 'required', 'source_start': 0},
    {'id': 'r02', 'quote': 'Coach engineers.', 'parent': 'Coach engineers.', 'importance': 'required', 'source_start': 28}]}


def assessment(cid, support):
    cites = [] if support == 'not-established' else [{'id': 'scope.default', 'quote': '$4M budget'}]
    return {'id': cid, 'support': support, 'citations': cites, 'boundary': 'Bounded.'}


def make_run(root, approved=True, status='complete-unreviewed'):
    run = root / 'run'
    run.mkdir()
    jd, clauses = JD.encode(), json.dumps(CLAUSES).encode()
    (run / 'jd.txt').write_bytes(jd)
    (run / 'baseline.md').write_text(BASELINE)
    (run / 'supplied-clauses.json').write_bytes(clauses)
    approval = {'reviewer': 'owner', 'jd_sha256': advise.digest(jd), 'clauses_sha256': advise.digest(clauses)} if approved else None
    (run / 'manifest.json').write_text(json.dumps({'approval': approval, 'configuration': {'requested_model': 'frontier'}}))
    (run / 'status.json').write_text(json.dumps({'status': status}))
    (run / 'report.json').write_text(json.dumps({'assessments': [assessment('r01', 'direct'), assessment('r02', 'adjacent')]}))
    (run / 'r01').mkdir()
    (run / 'r01' / 'result.json').write_text(json.dumps({'served_model': 'ref-model'}))
    return run


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)

    def test_freeze_then_load(self):
        with contextlib.redirect_stdout(io.StringIO()):
            mc.freeze(make_run(self.root), self.root / 'fx')
        self.assertEqual(mc.load_fixture(self.root / 'fx'), (self.root / 'fx').resolve())
        ref = json.loads((self.root / 'fx' / 'reference.json').read_text())
        self.assertEqual(ref['served_model'], 'ref-model')

    def test_refuses_unapproved_or_incomplete_run(self):
        with self.assertRaises(ValueError):
            mc.freeze(make_run(self.root, approved=False), self.root / 'a')
        (self.root / 'run').rename(self.root / 'old')
        with self.assertRaises(ValueError):
            mc.freeze(make_run(self.root, status='failed'), self.root / 'b')

    def test_detects_edited_fixture(self):
        with contextlib.redirect_stdout(io.StringIO()):
            mc.freeze(make_run(self.root), self.root / 'fx')
        (self.root / 'fx' / 'reference.json').write_text('{"assessments": []}')
        with self.assertRaises(ValueError):
            mc.load_fixture(self.root / 'fx')

    def test_cli_names_refusal_and_missing_run_without_paths(self):
        for args, expected in [(['freeze', str(self.root / 'nope'), str(self.root / 'fx')], 'missing'),
                               (['compare', str(self.root / 'nope'), str(self.root / 'c')], 'missing')]:
            err = io.StringIO()
            with patch('sys.argv', ['model_compare'] + args), contextlib.redirect_stderr(err):
                self.assertEqual(mc.main(), 1)
            self.assertIn(expected, err.getvalue())
            self.assertNotIn(str(self.root), err.getvalue())
        err = io.StringIO()
        with patch('sys.argv', ['model_compare', 'freeze', str(make_run(self.root, approved=False)), str(self.root / 'fx')]), \
                contextlib.redirect_stderr(err):
            mc.main()
        self.assertIn('owner-approved', err.getvalue())

    def test_refuses_paths_inside_public_checkout(self):
        with self.assertRaises(ValueError):
            mc.private(Path(__file__).parent / 'fx')


class RunAndCompareTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        with contextlib.redirect_stdout(io.StringIO()):
            mc.freeze(make_run(self.root), self.root / 'fx')

    def run_candidate(self, answers):
        """answers: clause id -> proposal dict, or an exception to raise."""
        def fake_complete(client, directory, system, data, schema, validate):
            answer = answers[data['clause']['id']]
            if isinstance(answer, Exception):
                raise answer
            validate(answer)
            return answer
        with patch.object(mc, 'Client'), patch.object(mc.advise, 'complete', fake_complete):
            mc.run(self.root / 'fx', type('Cfg', (), {'public_metadata': lambda self: {'requested_model': 'local'}})(), self.root / 'cand')

    def test_compare_reports_stronger_weaker_and_failures(self):
        self.run_candidate({'r01': assessment('r01', 'adjacent'), 'r02': ValueError('bad')})
        summary = mc.compare(self.root / 'fx', self.root / 'cand')
        self.assertEqual(summary['candidate_weaker_than_reference'], {'r01': ['direct', 'adjacent']})
        self.assertEqual(summary['failed'], {'r02': 'validation'})
        self.assertEqual(summary['exact_agreement'], 0)

    def test_overclaim_is_flagged_and_agreement_counted(self):
        self.run_candidate({'r01': assessment('r01', 'direct'), 'r02': assessment('r02', 'direct')})
        summary = mc.compare(self.root / 'fx', self.root / 'cand')
        self.assertEqual(summary['candidate_stronger_than_reference'], {'r02': ['adjacent', 'direct']})
        self.assertEqual(summary['exact_agreement'], 1)
        self.assertEqual(summary['mean_citation_jaccard'], 1.0)

    def test_system_prompt_override_is_used_and_hashed(self):
        seen = []
        def fake_complete(client, directory, system, data, schema, validate):
            seen.append(system)
            validate(assessment(data['clause']['id'], 'direct'))
            return assessment(data['clause']['id'], 'direct')
        cfg = type('Cfg', (), {'public_metadata': lambda self: {}})()
        with patch.object(mc, 'Client'), patch.object(mc.advise, 'complete', fake_complete):
            mc.run(self.root / 'fx', cfg, self.root / 'a', system='EXPERIMENT PROMPT')
            mc.run(self.root / 'fx', cfg, self.root / 'b')
        self.assertEqual(seen[:2], ['EXPERIMENT PROMPT'] * 2)
        self.assertEqual(seen[2], mc.advise.SYSTEM)
        a = json.loads((self.root / 'a' / 'manifest.json').read_text())
        b = json.loads((self.root / 'b' / 'manifest.json').read_text())
        self.assertEqual(a['system_prompt_source'], 'system-file')
        self.assertEqual(b['system_prompt_source'], 'built-in')
        self.assertNotEqual(a['system_prompt_sha256'], b['system_prompt_sha256'])

    def test_partial_results_survive_an_interrupted_run(self):
        calls = []
        def fake_complete(client, directory, system, data, schema, validate):
            calls.append(data['clause']['id'])
            if len(calls) == 2:
                raise KeyboardInterrupt
            validate(assessment(data['clause']['id'], 'direct'))
            return assessment(data['clause']['id'], 'direct')
        with patch.object(mc, 'Client'), patch.object(mc.advise, 'complete', fake_complete):
            with self.assertRaises(KeyboardInterrupt):
                mc.run(self.root / 'fx', type('Cfg', (), {'public_metadata': lambda self: {}})(), self.root / 'cand')
        self.assertEqual(list(json.loads((self.root / 'cand' / 'results.json').read_text())), ['r01'])
        with self.assertRaises(mc.Refusal):
            mc.compare(self.root / 'fx', self.root / 'cand')

    def test_mismatched_clause_set_is_rejected(self):
        self.run_candidate({'r01': assessment('r01', 'direct'), 'r02': assessment('r02', 'adjacent')})
        results = json.loads((self.root / 'cand' / 'results.json').read_text())
        del results['r02']
        (self.root / 'cand' / 'results.json').write_text(json.dumps(results))
        with self.assertRaises(ValueError):
            mc.compare(self.root / 'fx', self.root / 'cand')


PROBES = {'probes': [
    {'id': 'r001', 'category': 'negative', 'text': 'Experience with Kafka streaming.', 'absent_terms': ['Kafka']},
    {'id': 'r002', 'category': 'trap', 'text': 'Owned the profit and loss of a unit.'},
    {'id': 'r003', 'category': 'control', 'text': 'Managed a $4M budget.', 'citations': ['scope.default']}]}


class ProbeBase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        (self.root / 'baseline.md').write_text(BASELINE)
        self.spec = self.root / 'spec.json'

    def build(self, spec=PROBES, name='fx'):
        self.spec.write_text(json.dumps(spec))
        with contextlib.redirect_stdout(io.StringIO()):
            mc.build_probe(self.spec, self.root / 'baseline.md', self.root / name)
        return self.root / name


class ProbeTests(ProbeBase):
    def test_builds_loadable_probe_fixture(self):
        fx = self.build()
        self.assertEqual(mc.fixture_kind(fx), 'probe')
        mc.load_fixture(fx)
        self.assertEqual(json.loads((fx / 'expected.json').read_text())['r001']['acceptable'], ['not-established'])

    def test_negative_must_be_absent_from_baseline(self):
        bad = {'probes': [dict(PROBES['probes'][0], absent_terms=['budget'])]}
        with self.assertRaises(mc.Refusal):
            self.build(bad)

    def test_negative_requires_absent_terms_and_control_needs_known_ids(self):
        with self.assertRaises(mc.Refusal):
            self.build({'probes': [{'id': 'r001', 'category': 'negative', 'text': 'Anything.'}]})
        with self.assertRaises(mc.Refusal):
            self.build({'probes': [dict(PROBES['probes'][2], citations=['nope.default'])]}, name='fy')

    def test_scores_false_support_overclaim_and_underclaim(self):
        fx = self.build()
        ok = lambda support, cites=(): {'status': 'ok', 'support': support, 'citations': list(cites)}
        summary = mc.compare_probe(fx, {'r001': ok('adjacent', ['scope.default']), 'r002': ok('direct', ['scope.default']),
                                        'r003': ok('adjacent', ['scope.default'])})
        self.assertEqual(summary['false_support_on_negatives'], {'r001': 'adjacent'})
        self.assertEqual(summary['direct_on_near_misses_or_traps'], {'r002': ['trap', 'direct']})
        self.assertEqual(summary['controls_not_direct'], {'r003': 'adjacent'})

    def test_clean_candidate_and_miscited_control(self):
        fx = self.build()
        ok = lambda support, cites=(): {'status': 'ok', 'support': support, 'citations': list(cites)}
        clean = mc.compare_probe(fx, {'r001': ok('not-established'), 'r002': ok('adjacent', ['scope.default']),
                                      'r003': ok('direct', ['scope.default'])})
        self.assertEqual(clean['false_support_on_negatives'], {})
        self.assertEqual(clean['controls_direct_without_expected_citation'], [])
        self.assertEqual(clean['by_category']['negative']['as_expected'], 1)
        wrong = mc.compare_probe(fx, {'r001': ok('not-established'), 'r002': ok('not-established'),
                                      'r003': {'status': 'ok', 'support': 'direct', 'citations': []}})
        self.assertEqual(wrong['controls_direct_without_expected_citation'], ['r003'])

    def test_failed_answers_are_reported_not_scored(self):
        fx = self.build()
        summary = mc.compare_probe(fx, {'r001': {'status': 'failed', 'category': 'validation'},
                                        'r002': {'status': 'failed', 'category': 'timeout'},
                                        'r003': {'status': 'failed', 'category': 'validation'}})
        self.assertEqual(summary['by_category']['negative'], {'n': 1, 'valid': 0, 'as_expected': 0})
        self.assertEqual(len(summary['failed']), 3)


class NearMissTests(ProbeBase):
    SPEC = {'probes': [
        {'id': 'r001', 'category': 'near-miss', 'text': 'Owned a Kafka-based platform.', 'absent_terms': ['Kafka']},
        {'id': 'r002', 'category': 'negative', 'text': 'Holds a PMP certificate.', 'absent_terms': ['PMP']},
        {'id': 'r003', 'category': 'control', 'text': 'Managed a $4M budget.', 'citations': ['scope.default']}]}

    def score(self, a, b):
        fx = self.build(self.SPEC)
        ok = lambda support: {'status': 'ok', 'support': support, 'citations': ['scope.default']}
        return mc.compare_probe(fx, {'r001': ok(a), 'r002': ok(b), 'r003': ok('direct')})

    def test_adjacent_is_acceptable_for_near_miss_but_not_for_negative(self):
        summary = self.score('adjacent', 'adjacent')
        self.assertEqual(summary['by_category']['near-miss']['as_expected'], 1)
        self.assertEqual(summary['false_support_on_negatives'], {'r002': 'adjacent'})
        self.assertEqual(summary['direct_on_near_misses_or_traps'], {})

    def test_direct_on_near_miss_is_flagged(self):
        summary = self.score('direct', 'not-established')
        self.assertEqual(summary['direct_on_near_misses_or_traps'], {'r001': ['near-miss', 'direct']})
        self.assertEqual(summary['by_category']['near-miss']['as_expected'], 0)

    def test_near_miss_requires_absent_terms_present_in_no_baseline_text(self):
        with self.assertRaises(mc.Refusal):
            self.build({'probes': [{'id': 'r001', 'category': 'near-miss', 'text': 'Anything.'}]})
        with self.assertRaises(mc.Refusal):
            self.build({'probes': [dict(self.SPEC['probes'][0], absent_terms=['budget'])]}, name='fy')


class RejectedClaimTests(ProbeBase):
    def reject(self, cand, cid, support, valid_shape=True):
        (cand / cid).mkdir(parents=True)
        body = {'id': cid, 'support': support, 'citations': [], 'boundary': 'Private explanation text.'}
        if not valid_shape:
            body = 'not json'
        env = {'model': 'm', 'choices': [{'finish_reason': 'stop', 'message': {'role': 'assistant', 'content': json.dumps(body)}}]}
        (cand / cid / 'response.raw.json').write_text(json.dumps(env))

    def test_reads_only_the_support_label_of_a_rejected_answer(self):
        cand = self.root / 'cand'
        self.reject(cand, 'r001', 'direct')
        self.reject(cand, 'r002', 'bogus')
        self.reject(cand, 'r003', 'direct', valid_shape=False)
        self.assertEqual(mc.claimed_support(cand, 'r001'), 'direct')
        self.assertIsNone(mc.claimed_support(cand, 'r002'))
        self.assertIsNone(mc.claimed_support(cand, 'r003'))
        self.assertIsNone(mc.claimed_support(cand, 'missing'))

    def test_probe_compare_flags_rejected_direct_on_a_trap_but_not_on_a_control(self):
        fx = self.build()
        cand = self.root / 'cand'
        self.reject(cand, 'r002', 'direct')
        self.reject(cand, 'r003', 'direct')
        fail = lambda: {'status': 'failed', 'category': 'validation'}
        summary = mc.compare_probe(fx, {'r001': {'status': 'ok', 'support': 'not-established', 'citations': []},
                                        'r002': fail(), 'r003': fail()}, cand)
        self.assertEqual(summary['rejected_claiming_direct'], ['r002', 'r003'])
        self.assertEqual(summary['rejected_overclaims_on_negatives_near_misses_traps'], {'r002': ['trap', 'direct']})

    def test_unreadable_rejection_is_not_counted(self):
        fx = self.build()
        fail = {'status': 'failed', 'category': 'timeout'}
        summary = mc.compare_probe(fx, {'r001': fail, 'r002': fail, 'r003': fail}, self.root / 'empty')
        self.assertEqual(summary['rejected_claiming_direct'], [])


if __name__ == '__main__':
    unittest.main()
