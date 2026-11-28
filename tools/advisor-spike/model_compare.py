"""Freeze approved frontier runs as fixtures, rerun them on another model, compare against the reference.

The frontier result is a reference, not ground truth: agreement with it is evidence, disagreement needs reading.
Fixtures and runs hold personal data and must live outside this checkout.

A "probe" fixture is different: its labels are assistant-authored expectations (negative, trap, control), not
frontier output. It measures false support, which the frontier fixtures cannot because they have no negatives.
"""
import argparse
import json
from pathlib import Path
import sys
import time

import advise
from advise import assessment_schema, digest, parse_baseline, save, validate_assessment, validate_clauses, write_new
from evaluate import decode_json
from llm_client import Client, ClientError, Config, decode_response

class Refusal(ValueError):
    """A refusal raised by this module; its fixed message is safe to print."""


RANK = {'not-established': 0, 'adjacent': 1, 'direct': 2}
FILES = ['baseline.md', 'jd.txt', 'clauses.json', 'approval.json', 'reference.json']


def private(path):
    path = Path(path).expanduser().resolve()
    if path.is_relative_to(Path(__file__).resolve().parents[2]):
        raise Refusal('Fixtures and runs must be outside the public checkout')
    return path


def served_model(run_dir):
    for p in sorted(run_dir.glob('*/result.json')):
        return json.loads(p.read_text()).get('served_model')


def freeze(run_dir, fixture):
    run_dir, fixture = private(run_dir), private(fixture)
    status = json.loads((run_dir / 'status.json').read_text())
    manifest = json.loads((run_dir / 'manifest.json').read_text())
    if status.get('status') != 'complete-unreviewed' or not manifest.get('approval'):
        raise Refusal('Only a complete run assessed from an owner-approved clause file can be a fixture')
    report = json.loads((run_dir / 'report.json').read_text())
    clauses = (run_dir / 'supplied-clauses.json').read_bytes()
    jd = (run_dir / 'jd.txt').read_bytes()
    if manifest['approval']['clauses_sha256'] != digest(clauses) or manifest['approval']['jd_sha256'] != digest(jd):
        raise Refusal('Approval does not match the saved JD and clauses')
    fixture.mkdir(mode=0o700)
    write_new(fixture / 'baseline.md', (run_dir / 'baseline.md').read_bytes())
    write_new(fixture / 'jd.txt', jd)
    write_new(fixture / 'clauses.json', clauses)
    save(fixture / 'approval.json', manifest['approval'])
    save(fixture / 'reference.json', {'served_model': served_model(run_dir), 'configuration': manifest['configuration'],
                                      'assessments': report['assessments']})
    save(fixture / 'fixture.json', {'files': {n: digest((fixture / n).read_bytes()) for n in FILES},
                                    'source_run': run_dir.name, 'created': time.strftime('%Y-%m-%d')})
    print(f'Frozen {len(report["assessments"])} reference assessments.')


def fixture_kind(fixture):
    return json.loads((fixture / 'fixture.json').read_text()).get('kind', 'reference')


def load_fixture(fixture):
    fixture = private(fixture)
    recorded = json.loads((fixture / 'fixture.json').read_text())['files']
    if any(digest((fixture / n).read_bytes()) != h for n, h in recorded.items()):
        raise Refusal('Fixture file changed since it was frozen')
    if fixture_kind(fixture) == 'reference':
        approval = json.loads((fixture / 'approval.json').read_text())
        if (approval['jd_sha256'] != digest((fixture / 'jd.txt').read_bytes())
                or approval['clauses_sha256'] != digest((fixture / 'clauses.json').read_bytes())):
            raise Refusal('Approval does not match the fixture')
    return fixture


# negative: nothing related in the baseline. near-miss: the key item is absent but related content exists.
# trap: wording that inflates real evidence. control: plainly supported.
CATEGORIES = {'negative': ('not-established',), 'near-miss': ('adjacent', 'not-established'),
              'trap': ('adjacent', 'not-established'), 'control': ('direct',)}


def build_probe(spec_path, baseline_path, fixture):
    """Build a probe fixture from a private spec. Negatives must not appear anywhere in the baseline text."""
    spec_path, baseline_path, fixture = private(spec_path), private(baseline_path), private(fixture)
    spec = json.loads(spec_path.read_text(encoding='utf-8'))
    baseline_bytes = baseline_path.read_bytes()
    evidence, _ = parse_baseline(baseline_bytes.decode('utf-8'))
    known = {e['id'] for e in evidence}
    haystack = baseline_bytes.decode('utf-8').lower()
    jd, clauses, expected = 'Synthetic probe requirements, not a real posting.\n', [], {}
    for row in spec['probes']:
        category = row['category']
        if category not in CATEGORIES:
            raise Refusal('Unknown probe category')
        for term in row.get('absent_terms', []):
            if term.lower() in haystack:
                raise Refusal(f'Probe {row["id"]}: a term that must be absent occurs in the baseline')
        if category in ('negative', 'near-miss') and not row.get('absent_terms'):
            raise Refusal(f'Probe {row["id"]}: a {category} needs absent_terms to prove the item is absent')
        if any(c not in known for c in row.get('citations', [])) or (category == 'control' and not row.get('citations')):
            raise Refusal(f'Probe {row["id"]}: control needs known evidence IDs; unknown ID given')
        jd += '\n'
        start = len(jd)
        jd += row['text'] + '\n'
        clauses.append({'id': row['id'], 'quote': row['text'], 'parent': row['text'],
                        'importance': row.get('importance', 'required'), 'source_start': start})
        expected[row['id']] = {'label': row.get('label', row['id']), 'category': category, 'acceptable': list(CATEGORIES[category]),
                               'citations': row.get('citations', []), 'note': row.get('note', '')}
    validate_clauses({'clauses': clauses}, jd)
    fixture.mkdir(mode=0o700)
    write_new(fixture / 'baseline.md', baseline_bytes)
    write_new(fixture / 'jd.txt', jd)
    save(fixture / 'clauses.json', {'clauses': clauses})
    save(fixture / 'expected.json', expected)
    files = ['baseline.md', 'jd.txt', 'clauses.json', 'expected.json']
    save(fixture / 'fixture.json', {'kind': 'probe', 'files': {n: digest((fixture / n).read_bytes()) for n in files},
                                    'labels': 'assistant-authored expectations, not owner-reviewed or frontier-validated',
                                    'created': time.strftime('%Y-%m-%d')})
    print(f'Built probe fixture: {len(clauses)} probes.')


def run(fixture, config, out, system=None):
    """Assess every approved clause; a bad answer is recorded and the run continues.

    `system` replaces the assessment prompt for a prompt experiment; its hash is recorded in the manifest.
    """
    prompt = advise.SYSTEM if system is None else system
    fixture, out = load_fixture(fixture), private(out)
    jd = (fixture / 'jd.txt').read_text(encoding='utf-8')
    clauses = decode_json((fixture / 'clauses.json').read_text(encoding='utf-8'))
    validate_clauses(clauses, jd)
    evidence, _ = parse_baseline((fixture / 'baseline.md').read_text(encoding='utf-8'))
    client = Client(config)
    out.mkdir(mode=0o700)
    save(out / 'manifest.json', {'fixture': fixture.name, 'configuration': config.public_metadata(),
                                 'system_prompt_sha256': digest(prompt.encode('utf-8')),
                                 'system_prompt_source': 'built-in' if system is None else 'system-file'})
    results = {}
    for clause in clauses['clauses']:
        if clause['importance'] == 'context':
            continue
        cid = clause['id']
        try:
            proposal = advise.complete(client, out / cid, prompt,
                {'evidence': evidence, 'clause': clause,
                 'preceding_source_context': jd[:clause.get('source_start', jd.find(clause['parent']))]},
                assessment_schema(cid, evidence), lambda p, c=cid: validate_assessment(p, c, evidence))
            results[cid] = {'status': 'ok', 'support': proposal['support'],
                            'citations': [c['id'] for c in proposal['citations']]}
        except (ClientError, ValueError) as error:
            # Category only: exception text can carry model or private content.
            results[cid] = {'status': 'failed', 'category': getattr(error, 'category', 'validation')}
        print(cid, results[cid]['status'], results[cid].get('support', results[cid].get('category')), flush=True)
        # Rewritten after every clause so an interrupted run keeps its finished answers; compare refuses an incomplete set.
        advise.publish(out / 'results.json', results)


def claimed_support(candidate, cid):
    """The support label a rejected answer asserted, read from its saved raw response; None if unreadable.

    Only the enum value is returned, never the model's text. A rejected answer is not repaired or counted as valid.
    """
    try:
        proposal, _ = decode_response((candidate / cid / 'response.raw.json').read_bytes())
        value = proposal.get('support') if isinstance(proposal, dict) else None
    except (OSError, ValueError, ClientError, AttributeError):
        return None
    return value if value in RANK else None


def rejected_claims(candidate, results):
    return {c: claimed_support(candidate, c) for c, r in results.items() if r['status'] == 'failed'}


def compare_probe(fixture, results, candidate=None):
    expected = json.loads((fixture / 'expected.json').read_text())
    if set(results) != set(expected):
        raise Refusal('Candidate and reference cover different clauses')
    by = {c: {'n': 0, 'valid': 0, 'as_expected': 0} for c in CATEGORIES}
    false_support, overclaim, underclaim, miscited, failed = {}, {}, {}, [], {}
    for cid, exp in expected.items():
        row, stats = results[cid], by[exp['category']]
        stats['n'] += 1
        if row['status'] != 'ok':
            failed[cid] = row['category']
            continue
        stats['valid'] += 1
        stats['as_expected'] += row['support'] in exp['acceptable']
        if exp['category'] == 'negative' and row['support'] != 'not-established':
            false_support[cid] = row['support']
        if exp['category'] in ('near-miss', 'trap') and row['support'] == 'direct':
            overclaim[cid] = [exp['category'], row['support']]
        if exp['category'] == 'control':
            if row['support'] != 'direct':
                underclaim[cid] = row['support']
            elif not set(row['citations']) & set(exp['citations']):
                miscited.append(cid)
    claims = rejected_claims(candidate, results) if candidate else {}
    would_overclaim = {c: [expected[c]['category'], v] for c, v in claims.items()
                       if v == 'direct' and expected[c]['category'] in ('negative', 'near-miss', 'trap')}
    return {'kind': 'probe', 'by_category': by, 'failed': failed,
            'rejected_claiming_direct': sorted(c for c, v in claims.items() if v == 'direct'),
            'rejected_overclaims_on_negatives_near_misses_traps': would_overclaim,
            'false_support_on_negatives': false_support, 'direct_on_near_misses_or_traps': overclaim,
            'controls_not_direct': underclaim, 'controls_direct_without_expected_citation': miscited,
            'note': 'Labels are assistant-authored; read each disagreement before blaming the model.'}


def compare(fixture, candidate, report_path=None):
    fixture, candidate = load_fixture(fixture), private(candidate)
    if fixture_kind(fixture) == 'probe':
        summary = compare_probe(fixture, json.loads((candidate / 'results.json').read_text()), candidate)
        if report_path:
            save(private(report_path), summary)
        return summary
    reference = {a['id']: a for a in json.loads((fixture / 'reference.json').read_text())['assessments']}
    results = json.loads((candidate / 'results.json').read_text())
    if set(results) != set(reference):
        raise Refusal('Candidate and reference cover different clauses')
    ok = {c: r for c, r in results.items() if r['status'] == 'ok'}
    rows = {'agree': [], 'over': [], 'under': []}
    jaccard = []
    for cid, cand in ok.items():
        ref = reference[cid]
        d = RANK[cand['support']] - RANK[ref['support']]
        rows['agree' if d == 0 else 'over' if d > 0 else 'under'].append(cid)
        a, b = set(cand['citations']), {c['id'] for c in ref['citations']}
        if a | b:
            jaccard.append(len(a & b) / len(a | b))
    summary = {'clauses': len(reference), 'valid_answers': len(ok),
               'failed': {c: r['category'] for c, r in results.items() if r['status'] == 'failed'},
               'rejected_claiming_direct': sorted(c for c, v in rejected_claims(candidate, results).items() if v == 'direct'),
               'exact_agreement': len(rows['agree']),
               'candidate_stronger_than_reference': {c: [reference[c]['support'], ok[c]['support']] for c in rows['over']},
               'candidate_weaker_than_reference': {c: [reference[c]['support'], ok[c]['support']] for c in rows['under']},
               'mean_citation_jaccard': round(sum(jaccard) / len(jaccard), 3) if jaccard else None}
    if report_path:
        save(private(report_path), summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='cmd', required=True)
    f = sub.add_parser('freeze', help='copy an approved complete run into a new fixture directory')
    f.add_argument('run_dir')
    f.add_argument('fixture')
    b = sub.add_parser('build-probe', help='build a negative/trap/control probe fixture from a private spec')
    b.add_argument('spec')
    b.add_argument('baseline')
    b.add_argument('fixture')
    r = sub.add_parser('run', help='assess a fixture with another model config')
    r.add_argument('fixture')
    r.add_argument('--config', required=True, type=Path)
    r.add_argument('--out', required=True)
    r.add_argument('--system-file', type=Path, help='assessment prompt to use instead of the built-in one (prompt experiments)')
    c = sub.add_parser('compare', help='compare a candidate run with the fixture reference')
    c.add_argument('fixture')
    c.add_argument('candidate')
    c.add_argument('--report')
    args = parser.parse_args()
    try:
        if args.cmd == 'freeze':
            freeze(args.run_dir, args.fixture)
        elif args.cmd == 'build-probe':
            build_probe(args.spec, args.baseline, args.fixture)
        elif args.cmd == 'run':
            run(args.fixture, Config.load(args.config), args.out,
                None if args.system_file is None else args.system_file.read_text(encoding='utf-8'))
        else:
            print(json.dumps(compare(args.fixture, args.candidate, args.report), indent=1))
    except (ClientError, ValueError, OSError, KeyError) as error:
        # Only fixed messages are shown; other exception text can carry private content.
        reason = (str(error) if isinstance(error, Refusal)
                  else 'a required file or directory is missing (is the run finished?)' if isinstance(error, FileNotFoundError)
                  else type(error).__name__)
        print(f'model_compare failed: {reason}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
