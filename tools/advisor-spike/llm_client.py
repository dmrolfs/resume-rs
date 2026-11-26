"""Bounded OpenAI-compatible transport. No retries, redirects or implicit credentials."""
from dataclasses import dataclass, field
from http.client import HTTPConnection, HTTPException, HTTPSConnection
import ipaddress
import json
import math
import os
from pathlib import Path
import re
import signal
import ssl
import threading
import time
import tomllib
from urllib.parse import urlsplit

from evaluate import decode_json

MAX_RESPONSE = 2_000_000
MAX_REQUEST = 160_000
CHUNK = 65_536


class ClientError(ValueError):
    """Safe-to-display transport/configuration error (never includes response bodies).

    `category` is a fixed allowlisted label and `status` an HTTP status or None, so failures can be
    recorded durably without keeping server text.
    """

    def __init__(self, message, category='config', status=None):
        super().__init__(message)
        self.category = category
        self.status = status


def base_url(value):
    if not isinstance(value, str) or any(c.isspace() or ord(c) < 32 for c in value):
        raise ClientError('base_url must be an HTTP(S) URL without whitespace')
    try:
        parts = urlsplit(value)
        port = parts.port
        if port == 0:
            raise ValueError('Port zero is not an endpoint')
    except ValueError:
        raise ClientError('Invalid endpoint URL') from None
    if (parts.scheme not in ('http', 'https') or not parts.hostname or parts.username is not None
            or parts.password is not None or parts.query or parts.fragment
            or '%' in parts.netloc or '\\' in value or '..' in parts.path.split('/')):
        raise ClientError('Endpoint must have a host and no credentials, query, fragment or traversal')
    host = parts.hostname.lower()
    try:
        local = ipaddress.ip_address(host).is_loopback
    except ValueError:
        # A hostname (including localhost) could resolve elsewhere; use a literal for plaintext.
        local = False
    if parts.scheme == 'http' and not local:
        raise ClientError('Plain HTTP is allowed only for literal loopback addresses')
    host = f'[{host}]' if ':' in host else host
    default_port = 443 if parts.scheme == 'https' else 80
    authority = host + (f':{port}' if port and port != default_port else '')
    return f'{parts.scheme}://{authority}{parts.path.rstrip("/")}', local


@dataclass(frozen=True)
class Config:
    base_url: str
    model: str
    allow_remote: bool = False
    api_key_env: str | None = None
    credential_base_url: str | None = None
    ca_file: str | None = None
    engagement: str | None = None
    idle_timeout: int = 120
    total_timeout: int = 300
    max_tokens: int = 4096
    temperature: float | None = None
    response_format: str = 'json_schema'
    _local: bool = field(default=False, repr=False, init=False)

    def __post_init__(self):
        normalized, local = base_url(self.base_url)
        object.__setattr__(self, 'base_url', normalized)
        object.__setattr__(self, '_local', local)
        if type(self.allow_remote) is not bool or (not local and not self.allow_remote):
            raise ClientError('Remote endpoint requires allow_remote = true')
        if not isinstance(self.model, str) or not self.model.strip() or len(self.model) > 200 or any(ord(c) < 32 for c in self.model):
            raise ClientError('model must be a nonempty model ID')
        for name, low, high in [('idle_timeout', 1, 600), ('total_timeout', 1, 3600), ('max_tokens', 128, 32768)]:
            value = getattr(self, name)
            if type(value) is not int or not low <= value <= high:
                raise ClientError(f'{name} must be an integer from {low} to {high}')
        if self.temperature is not None and (type(self.temperature) not in (int, float)
                or not math.isfinite(self.temperature) or not 0 <= self.temperature <= 2):
            raise ClientError('temperature must be a finite number between 0 and 2')
        if self.response_format not in ('json_schema', 'json_object'):
            raise ClientError('response_format must be json_schema or json_object')
        if self.api_key_env is not None:
            if not isinstance(self.api_key_env, str) or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', self.api_key_env):
                raise ClientError('api_key_env must name an environment variable, not contain a key')
            if self.credential_base_url is None or base_url(self.credential_base_url)[0] != normalized:
                raise ClientError('credential_base_url must match base_url before credentials can be used')
        elif self.credential_base_url is not None:
            raise ClientError('credential_base_url requires api_key_env')
        if self.ca_file is not None and (not isinstance(self.ca_file, str) or not self.ca_file):
            raise ClientError('ca_file must be a path')
        if self.engagement is not None:
            if (not isinstance(self.engagement, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,127}', self.engagement)
                    or not self.api_key_env):
                raise ClientError('engagement requires a bound credential and a valid identifier')

    @classmethod
    def load(cls, path):
        try:
            data = tomllib.loads(Path(path).read_text())
        except (OSError, ValueError):
            raise ClientError('Cannot read valid TOML configuration') from None
        allowed = {name for name, f in cls.__dataclass_fields__.items() if f.init}
        if set(data) - allowed:
            raise ClientError('Unknown configuration keys: ' + ', '.join(sorted(set(data) - allowed)))
        try:
            return cls(**data)
        except TypeError:
            raise ClientError('Configuration requires base_url and model with valid field types') from None

    def public_metadata(self):
        return {'base_url': self.base_url, 'requested_model': self.model,
                'remote_consent': self.allow_remote, 'credential_used': self.api_key_env is not None,
                'custom_ca': self.ca_file is not None, 'engagement': self.engagement,
                'idle_timeout': self.idle_timeout, 'total_timeout': self.total_timeout, 'max_tokens': self.max_tokens,
                'temperature': self.temperature, 'response_format': self.response_format}


class Client:
    def __init__(self, config):
        self.config = config
        self._headers = {'Content-Type': 'application/json', 'Accept': 'application/json'}
        if config.api_key_env:
            key = os.environ.get(config.api_key_env, '')
            if not key or not key.isascii() or any(c.isspace() or ord(c) < 32 for c in key):
                raise ClientError('Configured credential environment variable is missing or invalid')
            self._headers['Authorization'] = 'Bearer ' + key
        if config.engagement:
            self._headers['x-engagement-id'] = config.engagement
        try:
            self._context = ssl.create_default_context()
            if config.ca_file:
                self._context.load_verify_locations(cafile=str(Path(config.ca_file).expanduser()))
        except (OSError, ValueError):
            raise ClientError('Cannot load TLS trust configuration', 'config') from None

    def payload(self, system, data, schema):
        # JSON-object-only servers still get the contract; validation is always local too.
        instruction = system + '\nReturn only JSON matching this schema:\n' + json.dumps(schema)
        fmt = ({'type': 'json_schema', 'json_schema': {'name': 'advisor_result', 'strict': True, 'schema': schema}}
               if self.config.response_format == 'json_schema' else {'type': 'json_object'})
        body = {'model': self.config.model, 'stream': False, 'max_tokens': self.config.max_tokens,
                'response_format': fmt, 'messages': [{'role': 'system', 'content': instruction},
                {'role': 'user', 'content': json.dumps(data, ensure_ascii=False)}]}
        if self.config.temperature is not None:
            body['temperature'] = self.config.temperature
        if len(json.dumps(body).encode()) > MAX_REQUEST:
            raise ClientError('Request exceeds byte limit; reduce input instead of silently truncating')
        return body

    def send(self, payload):
        data = json.dumps(payload).encode()
        if len(data) > MAX_REQUEST:
            raise ClientError('Request exceeds byte limit', 'request-limit')
        raw = self._exchange(data)
        # Do not persist a response that echoes the request credential.
        token = self._headers.get('Authorization', '').removeprefix('Bearer ')
        if token:
            screen_credential(raw, token)
        return raw

    def _exchange(self, data):
        """One attempt, bounded by a hard total deadline.

        A one-shot SIGALRM interrupts whatever blocking read or header parse is running, so buffered
        http.client reads cannot overrun the total. Signals only reach the main thread, so a caller on
        another thread is refused rather than silently unbounded.
        """
        cfg = self.config
        if threading.current_thread() is not threading.main_thread() or not hasattr(signal, 'setitimer'):
            raise ClientError('Total deadline needs the main thread on a POSIX system', 'config')

        class Expired(BaseException):
            pass

        def on_alarm(signum, frame):
            raise Expired

        previous = signal.signal(signal.SIGALRM, on_alarm)
        old_timer = signal.setitimer(signal.ITIMER_REAL, cfg.total_timeout)
        try:
            return self._attempt(data)
        except Expired:
            raise ClientError('Total request deadline exceeded; no retry performed', 'deadline') from None
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous)
            if old_timer[0]:  # give a caller's own pending timer back its remaining time
                signal.setitimer(signal.ITIMER_REAL, old_timer[0])

    def _attempt(self, data):
        cfg = self.config
        parts = urlsplit(cfg.base_url)
        # Proxy environment variables are never consulted: this connects directly to the configured host.
        conn = response = None
        try:
            if parts.scheme == 'https':
                conn = HTTPSConnection(parts.hostname, parts.port, timeout=cfg.idle_timeout, context=self._context)
            else:
                conn = HTTPConnection(parts.hostname, parts.port, timeout=cfg.idle_timeout)
            conn.connect()
            conn.request('POST', parts.path + '/chat/completions', body=data, headers=self._headers)
            response = conn.getresponse()
            if response.status != 200:
                # Redirects are not followed and bodies are never read or kept.
                raise ClientError(f'Endpoint returned HTTP {response.status}; no retry or redirect performed',
                                  'http-status', response.status)
            chunks, size = [], 0
            while True:
                chunk = response.read(CHUNK)
                if not chunk:
                    break
                size += len(chunk)
                if size > MAX_RESPONSE:
                    raise ClientError('Response exceeds byte limit', 'response-limit')
                chunks.append(chunk)
            return b''.join(chunks)
        except ClientError:
            raise
        except TimeoutError:
            raise ClientError('Endpoint timed out; no retry performed', 'timeout') from None
        except (OSError, ValueError, HTTPException, AttributeError):
            raise ClientError('Endpoint connection, TLS or read failed; no retry performed', 'connection') from None
        finally:
            # The response can own the socket once http.client detaches it from the connection.
            if response is not None:
                response.close()
            if conn is not None:
                conn.close()


def screen_credential(raw, token):
    """Reject literal and JSON-equivalent echoes before persisting provider output."""
    if token.encode() in raw:
        raise ClientError('Response echoed credential; response discarded', 'credential-echo')
    try:
        envelope = decode_json(raw)
    except (ValueError, TypeError):
        raise ClientError('Authenticated response is not valid JSON; response discarded', 'credential-echo') from None

    def check(value, depth=0):
        if depth > 32:
            raise ClientError('Response nesting exceeds inspection limit; response discarded', 'credential-echo')
        if isinstance(value, str):
            if token in value:
                raise ClientError('Response echoed credential; response discarded', 'credential-echo')
            # Content is itself serialized JSON inside the completion envelope.
            try:
                decoded = decode_json(value)
            except (ValueError, TypeError):
                return
            if decoded != value:
                check(decoded, depth + 1)
        elif isinstance(value, dict):
            for key, item in value.items():
                check(key, depth + 1)
                check(item, depth + 1)
        elif isinstance(value, list):
            for item in value:
                check(item, depth + 1)
    check(envelope)


def decode_response(raw):
    try:
        envelope = decode_json(raw)
        if not isinstance(envelope, dict) or not isinstance(envelope.get('model'), str):
            raise ValueError('Missing served model')
        choices = envelope.get('choices')
        if not isinstance(choices, list) or len(choices) != 1:
            raise ValueError('Require exactly one choice')
        choice = choices[0]
        message = choice['message']
        if (choice.get('finish_reason') != 'stop' or message.get('role') != 'assistant'
                or message.get('refusal') or message.get('tool_calls') or message.get('function_call')
                or not isinstance(message.get('content'), str)):
            raise ValueError('Incomplete, refused or non-text completion')
        proposal = decode_json(message['content'])
        if not isinstance(proposal, dict):
            raise ValueError('Proposal must be an object')
        return proposal, {'served_model': envelope['model'], 'usage': envelope.get('usage')}
    except (KeyError, TypeError, ValueError, AttributeError):
        raise ClientError('Invalid, refused or incomplete completion; no repair performed', 'completion') from None
