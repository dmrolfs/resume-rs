"""Synthetic tests only; no gateway traffic or personal artifacts."""
import copy
from http.client import BadStatusLine, IncompleteRead
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

import advise
import llm_client as llm

JD = 'Manage engineering budgets.'
BASELINE = '# Example\n\n## Summary\n\nManaged a $4M budget. <!-- =scope.default #org -->\n'
CLAUSES = {'clauses': [{'id': 'r01', 'quote': JD, 'parent': JD, 'importance': 'required'}]}
ASSESSMENT = {'id': 'r01', 'support': 'direct', 'citations': [{'id': 'scope.default', 'quote': '$4M budget'}], 'boundary': 'Budget ownership is explicit.'}


def envelope(proposal, **changes):
    result = {'model': 'served-test', 'choices': [{'finish_reason': 'stop', 'message': {'role': 'assistant', 'content': json.dumps(proposal)}}]}
    result.update(changes)
    return json.dumps(result).encode()


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.requests = []
        self.response = envelope(ASSESSMENT)
        self.code = 200
        outer = self
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers['Content-Length']))
                outer.requests.append((self.path, dict(self.headers), json.loads(body)))
                self.send_response(outer.code)
                if outer.code == 302:
                    self.send_header('Location', '/credential-trap')
                self.end_headers()
                self.wfile.write(outer.response)
            def log_message(self, *args):
                pass
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f'http://127.0.0.1:{self.server.server_port}/v1'

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_chat_contract_and_no_ambient_credentials(self):
        with patch.dict(os.environ, {'OPENAI_API_KEY': 'must-not-be-used', 'HTTPS_PROXY': 'https://invalid.invalid'}):
            client = llm.Client(llm.Config(self.url, 'test'))
            payload = client.payload('Instruction', {'input': 'data'}, {'type': 'object'})
            proposal, meta = llm.decode_response(client.send(payload))
        self.assertEqual(proposal, ASSESSMENT)
        self.assertEqual(meta['served_model'], 'served-test')
        path, headers, body = self.requests[0]
        self.assertEqual(path, '/v1/chat/completions')
        self.assertNotIn('Authorization', headers)
        self.assertEqual(body['response_format']['type'], 'json_schema')
        self.assertFalse(body['stream'])

    def test_bound_credential_and_object_mode(self):
        with patch.dict(os.environ, {'TEST_ADVISOR_KEY': 'synthetic-secret'}):
            client = llm.Client(llm.Config(self.url, 'test', api_key_env='TEST_ADVISOR_KEY', credential_base_url=self.url, response_format='json_object'))
            payload = client.payload('Instruction', {}, {'type': 'object'})
            client.send(payload)
        self.assertEqual(self.requests[0][1]['Authorization'], 'Bearer synthetic-secret')
        self.assertNotIn('synthetic-secret', json.dumps(payload))
        self.assertEqual(payload['response_format'], {'type': 'json_object'})

    def test_redirect_and_http_error_not_retried_or_echoed(self):
        for code in [302, 401, 403, 429, 500]:
            self.code = code
            self.response = b'secret server body'
            with self.assertRaises(llm.ClientError) as caught:
                llm.Client(llm.Config(self.url, 'test')).send({})
            self.assertNotIn('secret', str(caught.exception))
        self.assertEqual(len(self.requests), 5)

    def test_response_cap_and_credential_echo(self):
        self.response = b'x' * 21
        with patch.object(llm, 'MAX_RESPONSE', 20):
            with self.assertRaises(llm.ClientError):
                llm.Client(llm.Config(self.url, 'test')).send({})
        self.response = b'synthetic-secret'
        with patch.dict(os.environ, {'TEST_ADVISOR_KEY': 'synthetic-secret'}):
            client = llm.Client(llm.Config(self.url, 'test', api_key_env='TEST_ADVISOR_KEY', credential_base_url=self.url))
            with self.assertRaisesRegex(llm.ClientError, 'echoed credential'):
                client.send({})

    def test_connection_error_safe_and_single_attempt(self):
        client = llm.Client(llm.Config(self.url, 'test'))
        errors = [TimeoutError('sensitive text'), BadStatusLine('sensitive text'), IncompleteRead(b'sensitive text'), OSError('sensitive text')]
        for error in errors:
            with patch.object(llm, 'HTTPConnection') as conn:
                conn.return_value.connect.side_effect = error
                with self.assertRaises(llm.ClientError) as caught:
                    client.send({})
            self.assertNotIn('sensitive', str(caught.exception))
            self.assertIn(caught.exception.category, {'timeout', 'connection'})
            conn.assert_called_once()

    def test_http_failures_carry_status_not_body(self):
        for code in [302, 429, 500, 503]:
            self.code, self.response = code, b'secret server body'
            with self.assertRaises(llm.ClientError) as caught:
                llm.Client(llm.Config(self.url, 'test')).send({})
            self.assertEqual((caught.exception.category, caught.exception.status), ('http-status', code))
            self.assertNotIn('secret', str(caught.exception))

    def test_total_deadline_bounds_a_dripping_provider(self):
        # Real sockets: one byte every 0.2 s stays inside idle_timeout but must not outlast total_timeout.
        outer = self
        class Drip(BaseHTTPRequestHandler):
            def do_POST(self):
                self.rfile.read(int(self.headers['Content-Length']))
                self.send_response(200)
                self.send_header('Content-Length', '1000')
                self.end_headers()
                try:
                    for _ in range(1000):
                        self.wfile.write(b'x')
                        self.wfile.flush()
                        time.sleep(0.2)
                except OSError:
                    pass
            def log_message(self, *a):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), Drip)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            client = llm.Client(llm.Config(f'http://127.0.0.1:{server.server_port}/v1', 'm', idle_timeout=5, total_timeout=1))
            start = time.monotonic()
            with self.assertRaises(llm.ClientError) as caught:
                client.send({})
            elapsed = time.monotonic() - start
        finally:
            server.shutdown()
            server.server_close()
        self.assertEqual(caught.exception.category, 'deadline')
        self.assertLess(elapsed, 2.5)

    def test_deadline_stops_a_server_that_never_answers(self):
        class Silent(BaseHTTPRequestHandler):
            def do_POST(self):
                self.rfile.read(int(self.headers['Content-Length']))
                time.sleep(3)
            def log_message(self, *a):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), Silent)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            client = llm.Client(llm.Config(f'http://127.0.0.1:{server.server_port}/v1', 'm', idle_timeout=10, total_timeout=1))
            start = time.monotonic()
            with self.assertRaises(llm.ClientError) as caught:
                client.send({})
        finally:
            server.shutdown()
            server.server_close()
        self.assertEqual(caught.exception.category, 'deadline')
        self.assertLess(time.monotonic() - start, 2.5)

    def test_timer_and_handler_are_restored_even_after_failure(self):
        import signal
        before = signal.getsignal(signal.SIGALRM)
        self.code = 500
        with self.assertRaises(llm.ClientError):
            llm.Client(llm.Config(self.url, 'test')).send({})
        self.assertIs(signal.getsignal(signal.SIGALRM), before)
        self.assertEqual(signal.getitimer(signal.ITIMER_REAL)[0], 0)

    def test_non_main_thread_is_refused_not_left_unbounded(self):
        result = []
        def work():
            try:
                llm.Client(llm.Config(self.url, 'test')).send({})
            except llm.ClientError as error:
                result.append(error.category)
        thread = threading.Thread(target=work)
        thread.start()
        thread.join()
        self.assertEqual(result, ['config'])
        self.assertEqual(self.requests, [])

    def test_response_and_connection_closed_on_every_exit(self):
        class Response:
            def __init__(self, status, body=b'', fail=None):
                self.status, self.body, self.fail, self.closed = status, body, fail, False
            def read(self, n):
                if self.fail:
                    raise self.fail
                chunk, self.body = self.body[:n], self.body[n:]
                return chunk
            def close(self):
                self.closed = True
        for response in [Response(500), Response(200, b'x' * 21), Response(200, fail=OSError('sensitive')),
                         Response(200, fail=KeyboardInterrupt())]:
            conns = []
            class Conn:
                def __init__(self, *a, **k):
                    self.closed = False
                    conns.append(self)
                def connect(self): pass
                def request(self, *a, **k): pass
                def getresponse(self): return response
                def close(self): self.closed = True
            client = llm.Client(llm.Config(self.url, 'test'))
            with patch.object(llm, 'HTTPConnection', Conn), patch.object(llm, 'MAX_RESPONSE', 20):
                with self.assertRaises((llm.ClientError, KeyboardInterrupt)):
                    client.send({})
            self.assertTrue(response.closed and conns[0].closed)

    def test_body_closing_socket_is_handled(self):
        # http.client sets conn.sock to None after the final read; the client must not touch it then.
        self.response = envelope(ASSESSMENT)
        raw = llm.Client(llm.Config(self.url, 'test')).send({})
        self.assertEqual(raw, self.response)

    def test_encoded_credential_echoes_rejected(self):
        token = 'synthetic-secret'
        for raw in [b'{"model":"\\u0073ynthetic-secret"}',
                    json.dumps({'content': '{"boundary":"\\u0073ynthetic-secret"}'}).encode(),
                    json.dumps({'usage': {'detail': '\\u0073ynthetic-secret'}}).encode().replace(b'\\\\u0073', b'\\u0073')]:
            with self.assertRaises(llm.ClientError):
                llm.screen_credential(raw, token)


class ConfigurationTests(unittest.TestCase):
    def test_remote_consent_and_plaintext(self):
        for url in ['https://example.com/v1', 'http://example.com/v1', 'http://localhost:11434/v1']:
            with self.assertRaises(llm.ClientError):
                llm.Config(url, 'model')
        self.assertEqual(llm.Config('https://example.com/v1/', 'model', allow_remote=True).base_url, 'https://example.com/v1')
        llm.Config('http://[::1]:11434/v1', 'model')

    def test_malformed_endpoint_and_credential_binding(self):
        for url in ['https://key@example.com', 'https://example.com?key=x', 'https://example.com/#x', 'https://example.com:bad', 'https://example.com:0', 'http://127.0.0.1:0', 'https://example.com/a/../b', 'https://example.com/\n']:
            with self.assertRaises(llm.ClientError):
                llm.Config(url, 'model', allow_remote=True)
        with self.assertRaises(llm.ClientError):
            llm.Config('http://127.0.0.1:11434/v1', 'model', api_key_env='KEY', credential_base_url='https://example.com/v1')

    def test_settings_types_and_missing_key(self):
        for change in [{'idle_timeout': True}, {'total_timeout': 0}, {'max_tokens': -1}, {'temperature': float('nan')}, {'allow_remote': 'yes'}, {'response_format': 'auto'}, {'model': ''}]:
            with self.assertRaises(llm.ClientError):
                llm.Config(**{'base_url': 'http://127.0.0.1/v1', 'model': 'm', **change})
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(llm.ClientError):
                llm.Client(llm.Config('http://127.0.0.1/v1', 'm', api_key_env='KEY', credential_base_url='http://127.0.0.1/v1'))
        with self.assertRaises(llm.ClientError):
            llm.Client(llm.Config('http://127.0.0.1/v1', 'm', ca_file='/does-not-exist'))

    def test_config_file_and_request_limit(self):
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp) / 'config.toml'
            p.write_text('base_url = "http://127.0.0.1:11434/v1"\nmodel = "larger-local"\n')
            self.assertEqual(llm.Config.load(p).model, 'larger-local')
            p.write_text(p.read_text() + 'api_key = "forbidden"\n')
            with self.assertRaises(llm.ClientError):
                llm.Config.load(p)
        client = llm.Client(llm.Config('http://127.0.0.1/v1', 'm'))
        with patch.object(llm, 'MAX_REQUEST', 10):
            with self.assertRaises(llm.ClientError):
                client.payload('test', {}, {})


class ContractTests(unittest.TestCase):
    def test_initial_read_rejects_special_and_oversized_files(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'input'
            os.mkfifo(path)
            with self.assertRaisesRegex(ValueError, 'regular file'):
                advise.read_input(path)
            path.unlink()
            path.write_bytes(b'x' * (advise.MAX_INPUT + 1))
            with self.assertRaisesRegex(ValueError, 'byte limit'):
                advise.read_input(path)
            path.write_text('valid input')
            self.assertEqual(advise.read_input(path), (b'valid input', 'valid input'))

    def test_output_inside_checkout_is_rejected_before_input_reads(self):
        with patch.object(advise, 'read_input') as read:
            with self.assertRaisesRegex(ValueError, 'outside the public checkout'):
                advise.run(None, 'unused', 'unused', Path(advise.__file__).parent / 'runs')
            read.assert_not_called()

    def test_evidence_parsing_and_fail_closed_subset(self):
        evidence = advise.evidence_from_markdown(BASELINE)
        self.assertEqual(evidence[0]['text'], 'Managed a $4M budget.')
        for source in [BASELINE * 2, BASELINE.replace('#org', '@draft'), BASELINE.replace('#org', '@verify'), BASELINE.replace('scope.default', 'scope'), BASELINE.replace(' -->', ' --> trailing')]:
            with self.assertRaises(ValueError):
                advise.evidence_from_markdown(source)
        for source in ['```text\n\n' + BASELINE + '\n\n```', '> Example. <!-- =example.default #org -->',
                       '- Approved. <!-- =scope.default #org -->\n  Only a prototype.',
                       'Intro\n- Approved. <!-- =scope.default #org -->']:
            with self.assertRaises(ValueError):
                advise.evidence_from_markdown(source)
        with self.assertRaisesRegex(ValueError, 'default variant'):
            advise.evidence_from_markdown(BASELINE.replace('scope.default', 'scope.long'))
        evidence, context = advise.parse_baseline('Context line.\n\n' + BASELINE)
        self.assertEqual(context, ['Context line.'])
        self.assertEqual(advise.parse_baseline(BASELINE)[1], [])  # no state leaks between parses
        multi = BASELINE + '\n## Experience\n\n### Company\n\n**Era · 2025**\n\n- One. <!-- =one.default #org -->\n- Two. <!-- =two.default #org -->'
        rows = advise.evidence_from_markdown(multi)
        self.assertEqual([r['text'] for r in rows], ['Managed a $4M budget.', 'One.', 'Two.'])
        self.assertEqual(rows[-1]['role'], '### Company')
        self.assertEqual(rows[-1]['era'], '**Era · 2025**')

    def test_clauses_exact_unique(self):
        advise.validate_clauses(CLAUSES, JD)
        for changes in [{'quote': 'Invented'}, {'parent': 'Invented'}, {'id': '../x'}, {'importance': 'critical'}]:
            bad = copy.deepcopy(CLAUSES)
            bad['clauses'][0].update(changes)
            with self.assertRaises(ValueError):
                advise.validate_clauses(bad, JD)
        with self.assertRaises(ValueError):
            advise.validate_clauses({'clauses': CLAUSES['clauses'] * 2}, JD)

    def test_assessment_exact_citations(self):
        evidence = advise.evidence_from_markdown(BASELINE)
        advise.validate_assessment(ASSESSMENT, 'r01', evidence)
        for changes in [{'id': 'r02'}, {'support': 'maybe'}, {'citations': []}, {'citations': [{'id': 'scope.default', 'quote': 'Invented'}]}, {'citations': [{'id': 'wrong', 'quote': '$4M'}]}, {'citations': ASSESSMENT['citations'] * 2}, {'boundary': ' '}, {'extra': True}]:
            bad = {**ASSESSMENT, **changes}
            with self.assertRaises(ValueError):
                advise.validate_assessment(bad, 'r01', evidence)

    def test_refusal_truncation_duplicate_keys_and_tools_rejected(self):
        good = json.loads(envelope(ASSESSMENT))
        bad = copy.deepcopy(good)
        bad['choices'][0]['finish_reason'] = 'length'
        variants = [b'{"model":"a","model":"b"}', b'not json', json.dumps(bad).encode()]
        for field, value in [('refusal', 'No'), ('tool_calls', [{}]), ('content', None)]:
            bad = copy.deepcopy(good)
            bad['choices'][0]['message'][field] = value
            variants.append(json.dumps(bad).encode())
        for raw in variants:
            with self.assertRaises(llm.ClientError):
                llm.decode_response(raw)


def approval_for(jd, clauses):
    return {'reviewer': 'Synthetic reviewer', 'jd_sha256': advise.digest(jd.read_bytes()),
            'clauses_sha256': advise.digest(clauses.read_bytes())}


EXTRACTED = {'clauses': [{'segment': 1, 'quote': JD, 'importance': 'required'}]}


class EngineTests(unittest.TestCase):
    def test_staged_run_immutable_private_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out = root / 'resume.md', root / 'jd.txt', root / 'run'
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            with patch.object(llm.Client, 'send', return_value=envelope(EXTRACTED)) as send:
                advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)
                self.assertEqual(send.call_count, 1)
            self.assertEqual(baseline.read_text(), BASELINE)
            self.assertEqual(jd.read_text(), JD)
            self.assertEqual(out.stat().st_mode & 0o777, 0o700)
            self.assertFalse((out / 'report.json').exists())
            self.assertEqual(json.loads((out / 'status.json').read_text())['status'], 'awaiting-clause-review')
            self.assertIn('SEGMENT 1', (out / 'review.txt').read_text())
            with self.assertRaises(FileExistsError):
                advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)

    def test_invalid_generation_has_no_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out = root / 'resume.md', root / 'jd.txt', root / 'run'
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            with patch.object(llm.Client, 'send', return_value=envelope({'clauses': []})):
                with self.assertRaises(ValueError):
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)
            self.assertFalse((out / 'report.json').exists())
            self.assertEqual(json.loads((out / 'status.json').read_text())['status'], 'failed')

    def test_later_failure_preserves_diagnostics_not_complete_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out = root / 'r.md', root / 'jd.txt', root / 'run'
            baseline.write_text(BASELINE)
            jd.write_text(JD + ' Hire engineers.')
            clauses = copy.deepcopy(CLAUSES)
            clauses['clauses'].append({'id': 'r02', 'quote': 'Hire engineers.', 'parent': 'Hire engineers.', 'importance': 'required'})
            bad = {**ASSESSMENT, 'id': 'r02', 'citations': [{'id': 'invented', 'quote': 'x'}]}
            clause_path = root / 'approved.json'
            clause_path.write_text(json.dumps(clauses))
            with patch.object(llm.Client, 'send', side_effect=[envelope(ASSESSMENT), envelope(bad)]) as send:
                with self.assertRaises(ValueError):
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out, clause_path,
                               approval=approval_for(jd, clause_path))
                self.assertEqual(send.call_count, 2)
            self.assertTrue((out / 'r01/proposal.json').exists())
            self.assertFalse((out / 'report.json').exists())
            self.assertEqual(json.loads((out / 'status.json').read_text())['usable_as_complete_proposal'], False)

    def test_failure_status_is_allowlisted_and_stage_scoped(self):
        secret = 'PRIVATE-SERVER-TEXT'
        cases = [(llm.ClientError(secret, 'http-status', 500), 'http-status', 500), (ValueError(secret), 'validation', None)]
        for error, category, status in cases:
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                baseline, jd, out = root / 'r.md', root / 'jd.txt', root / 'run'
                baseline.write_text(BASELINE)
                jd.write_text(JD)
                with patch.object(llm.Client, 'send', side_effect=error):
                    with self.assertRaises(type(error)):
                        advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)
                record = json.loads((out / 'status.json').read_text())
                self.assertEqual((record['stage'], record['category'], record['http_status']), ('extraction:1', category, status))
                self.assertNotIn(secret, (out / 'status.json').read_text())
                self.assertFalse((out / 'status.json.tmp').exists())

    def test_interrupt_is_marked_failed_not_running(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out = root / 'r.md', root / 'jd.txt', root / 'run'
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            with patch.object(llm.Client, 'send', side_effect=KeyboardInterrupt):
                with self.assertRaises(KeyboardInterrupt):
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)
            record = json.loads((out / 'status.json').read_text())
            self.assertEqual((record['status'], record['category']), ('failed', 'internal'))
            self.assertFalse((out / 'report.json').exists())

    def test_supplied_clause_file_change_fails_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, clauses, out = [root / n for n in ['r.md', 'jd.txt', 'clauses.json', 'run']]
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            clauses.write_text(json.dumps(CLAUSES))
            def mutate(payload):
                clauses.write_text(json.dumps(CLAUSES) + ' ')
                return envelope(ASSESSMENT)
            with patch.object(llm.Client, 'send', side_effect=mutate):
                with self.assertRaises(ValueError):
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out, clauses,
                               approval=approval_for(jd, clauses))
            record = json.loads((out / 'status.json').read_text())
            self.assertTrue(record['inputs_changed_or_unreadable'])
            self.assertFalse((out / 'report.json').exists())

    def test_input_recheck_is_bounded_and_rejects_special_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            same, big, gone, fifo = root / 'same', root / 'big', root / 'gone', root / 'fifo'
            same.write_bytes(b'abc')
            big.write_bytes(b'abc' + b'x' * 10_000_000)
            os.mkfifo(fifo)
            self.assertTrue(advise.same_file(same, b'abc'))
            self.assertFalse(advise.same_file(same, b'abd'))
            self.assertFalse(advise.same_file(big, b'abc'))
            self.assertFalse(advise.same_file(gone, b'abc'))
            start = time.monotonic()
            self.assertFalse(advise.same_file(fifo, b'abc'))  # must not block on the pipe
            self.assertLess(time.monotonic() - start, 1)

    def test_failure_is_published_before_input_diagnostics(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out = root / 'r.md', root / 'jd.txt', root / 'run'
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            seen = []
            def spy(*args):
                seen.append(json.loads((out / 'status.json').read_text())['status'])
                raise ValueError('diagnostic failed')
            with patch.object(llm.Client, 'send', side_effect=llm.ClientError('x', 'timeout')), patch.object(advise, 'check_inputs', spy):
                with self.assertRaises(llm.ClientError):
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)
            self.assertEqual(seen, ['failed'])  # already failed when the diagnostic ran
            self.assertEqual(json.loads((out / 'status.json').read_text())['category'], 'timeout')

    def test_publication_failure_keeps_original_error(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out = root / 'r.md', root / 'jd.txt', root / 'run'
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            real = advise.publish
            calls = []
            def flaky(path, value):
                calls.append(value['status'])
                if value['status'] == 'failed':
                    raise OSError('disk full')
                real(path, value)
            with patch.object(llm.Client, 'send', side_effect=llm.ClientError('x', 'http-status', 500)), patch.object(advise, 'publish', flaky):
                with self.assertRaises(llm.ClientError) as caught:
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out)
            self.assertEqual(caught.exception.status, 500)
            self.assertEqual(json.loads((out / 'status.json').read_text())['status'], 'running')  # not complete

    def test_manifest_records_config_hash_and_runtime(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, out, cfg = root / 'r.md', root / 'jd.txt', root / 'run', root / 'c.toml'
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            cfg.write_text('base_url = "http://127.0.0.1/v1"\nmodel = "m"\n')
            with patch.object(llm.Client, 'send', return_value=envelope(EXTRACTED)):
                advise.run(llm.Config.load(cfg), baseline, jd, out, None, cfg)
            m = json.loads((out / 'manifest.json').read_text())
            self.assertEqual(m['configuration_file_sha256'], advise.digest(cfg.read_bytes()))
            self.assertIn('python', m['runtime'])
            self.assertEqual(m['configuration']['total_timeout'], 300)

    def test_supplied_clauses_skip_extraction_and_input_mutation_fails(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            baseline, jd, clauses, out = [root / n for n in ['r.md', 'jd.txt', 'clauses.json', 'run']]
            baseline.write_text(BASELINE)
            jd.write_text(JD)
            clauses.write_text(json.dumps(CLAUSES))
            def mutate(payload):
                jd.write_text('changed')
                return envelope(ASSESSMENT)
            with patch.object(llm.Client, 'send', side_effect=mutate) as send:
                with self.assertRaises(ValueError):
                    advise.run(llm.Config('http://127.0.0.1/v1', 'm'), baseline, jd, out, clauses,
                               approval=approval_for(jd, clauses))
                self.assertEqual(send.call_count, 1)
            self.assertFalse((out / 'report.json').exists())


if __name__ == '__main__':
    unittest.main()
