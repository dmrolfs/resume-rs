# Advisor spike preparation

Python 3.11+ standard library only.
This is a preparation and partial-evaluation tool for `rsm-eam.1`, not the advisor product.
It does not change Cargo, the renderer, original resumes, or existing output directories.

## Prepare a candidate

```sh
python3 prepare.py /absolute/private/consolidation-recipe.json /absolute/private/new-candidate-directory
```

The owner-approved private root for this evaluation is `~/Documents/vrm/my/job/advisor-spike/`.
Never place personal data in this code directory.
The destination parent must exist; the destination itself must not exist.
Outputs are private files in a newly created directory.
A write failure can leave a partial directory; do not treat it as a complete candidate or retry into it.

The private recipe explicitly lists source paths and SHA-256 hashes, candidate nodes and source line references, proposed omissions with reasons, and review decisions.
Every nonblank source line must occur exactly once across nodes and omissions.
Identical source lines are deduplicated within a node; multiple distinct bullets become one bullet containing their unchanged sentences in the specified order.
The tool does not choose equivalence, resolve conflicting facts, infer tags or rewrite claims.
Those choices are explicit in the recipe and require owner review.

Outputs:
- `Resume-Baseline.md`: candidate with annotations, not owner-approved content.
- `source-map.json`: original text and line/hash provenance for every nonblank source line, with candidate line references or proposed omission reasons.
- `recipe.json`: the exact input recipe.
- `decisions.json`: review decisions, including pending factual and wording questions.

Evidence IDs are scoped to this immutable source snapshot (`source-key` plus line number), not permanent IDs across revisions.
If originals change, preparation refuses the old recipe.
Use a new reviewed recipe and output directory rather than silently renumbering an existing evaluation.
The script validates only its recipe's annotation names, group adjacency, uniqueness and default presence.
Actual Markdown structure and annotation semantics must be validated by `resume tags` and `resume check`.

`@verify` does not prevent publishing: it keeps the content and reports it.
No output from this preparation phase should be sent as an application before owner review.
The preparation command makes no model calls, remote requests or model downloads.

## Local selection evaluation

```sh
python3 evaluate.py /absolute/private/candidate /absolute/private/jd.txt /absolute/private/new-run --model qwen3:8b
```

Requires an existing parent directory and a locally installed model served at `http://127.0.0.1:11434`.
Proxy environment settings and redirects are refused; cloud-tagged models and models reporting remote metadata are refused.
The harness itself contacts only loopback; this is not a network sandbox for the Ollama process.
No installation, remote fallback, retries or response repair occurs.

The complete candidate must match its source recipe, including headings and blank separators.
Evidence IDs and mappings must match original source text and hashes.
Whole variant groups are processed in bounded slices, sequentially, with their role/era context.
Summaries and items marked `@verify` or `@draft` are excluded; ungrouped content is not evaluated.
These exclusions are recorded, not evidence of missing experience.
Source text is not independent proof of an achievement.

Every slice saves its request, raw response bytes when received, result, and structurally valid proposal if any.
The run records model digest, settings, input hashes, runner version and harness hash.
Malformed, truncated-generation, invalid-reference and altered-quote responses are rejected, not silently repaired.
A setup or final-integrity failure may leave partial files without `summary.json`; such a run is invalid.

A `structure-valid` result proves only shape, exact reference/quote existence and matching source provenance.
It does not prove factual support or that the model saw the full input.
Context completeness is explicitly unverified; byte limits are not token accounting.
A gap applies only to its evidence slice and must not be reported as a global candidate deficiency.
There is no synthesis step, summary-prose generation, final spec emission or automatic modification of the baseline.
Thus these runs are diagnostic partial evaluations, not completion of the two-model spec-usefulness acceptance criterion.


## Fixed-ID diagnostic

```sh
python3 fixed_ids.py /absolute/private/candidate /absolute/private/jd.txt /absolute/private/case.json /absolute/private/expected.json /absolute/private/new-run --model qwen3:8b
```

Case JSON has `requirements` (R01-style IDs to exact JD excerpts) and `evidence` (E01-style IDs to group/variant objects).
Expected JSON separately maps every requirement to `status` and `acceptable` evidence IDs.
Statuses are `supported`, `partial`, and `not_evidenced`.
Use one shared evidence pool, including distractors and requirements with no support, and freeze labels before inference.
Expected labels are never passed to the request builder.

The model returns one answer for every requirement, with status, evidence IDs and an explanation capped at 300 characters.
ID enums constrain the schema, but explicit post-validation remains mandatory.
Evidence-present/absent agreement, exact status agreement and out-of-set citations are recorded separately against assistant-authored labels.
An ID-valid explanation can still misattribute work or infer unsupported experience; review the text manually.
This curated diagnostic is neither a complete application evaluation nor a causal comparison with the earlier slice runs.

## Configurable evidence advisor

`advise.py` is the first gateway/local evidence engine for `rsm-eam.3`.
It accepts the approved canonical annotated baseline directly, without the older consolidation recipe.
It extracts clauses from numbered source segments in batches targeting 900 characters, with at most 39 accepted clauses per batch.
Reaching the 40-item response cap is rejected as possibly incomplete rather than silently accepted.
A long indivisible segment stays intact and remains subject to the request byte cap.
The model returns only segment number, quote and importance; the tool assigns IDs, parents and Unicode-character source offsets.
Preceding source context accompanies batches and assessments to retain governing headings and list introductions.

Extraction stops at `awaiting-clause-review`, with `review.txt` grouped by segment and `clauses.json` for corrections.
It does not send resume evidence for assessment until a separate invocation supplies `--clauses` and `--approval`.
Review all omissions and required/preferred/context classifications before approving.
Context items remain in the artifacts but are not assessed.
Approved assessments use a stable schema and evidence-first payload to permit prefix caching, without guaranteeing cache hits.
The baseline remains unchanged; no final ranking or position spec is generated.

```sh
python3 advise.py --config /absolute/private/gateway.toml \
  --baseline /absolute/private/Resume-Baseline.md \
  --jd /absolute/private/jd.txt --out /absolute/private/new-run
```

Copy `advisor-gateway.example.toml` or `advisor-local.example.toml` outside the repository.
Set `base_url` to the OpenAI-compatible API root (usually ending in `/v1`) and choose `model`.
The client appends `/chat/completions`.
No Pi dependency or hard-coded gateway address/model is required.
Supply any bearer credential in the explicitly named environment variable using your existing credential manager.
The tool does not read Pi authentication files, refresh OAuth tokens, or invoke credential shell commands.
For expiring tokens, refresh them outside the tool before a run.
Never put a token in TOML or on the command line.

`credential_base_url` must match the configured endpoint before the named credential is used.
A local configuration omits both credential fields, so switching configurations does not send the gateway token to a local server.
Remote endpoints require HTTPS and `allow_remote = true`.
Plain HTTP is accepted only for literal loopback IPs, not hostnames.
TLS verification stays enabled; `ca_file` adds a private CA when needed.
The client connects directly to the configured host, so proxy environment variables are never consulted; redirects are not followed and bodies of error responses are never read or kept.
There are no automatic retries or provider fallback.
A loopback service can itself forward traffic; verify the service you operate if local-only processing matters.

`total_timeout` bounds socket transport from connect to last body byte using a process signal.
A strict wall-clock bound during platform DNS resolution has not been verified; do not treat it as a process-level watchdog.
It is enforced with a one-shot `SIGALRM` timer, so it interrupts a blocked read or a provider that drips bytes inside the idle window.
This means the engine runs only on the main thread of a POSIX system; any other caller is refused with a `config` error instead of running unbounded.
The timer and signal handler are restored afterwards.
`idle_timeout` additionally bounds each individual socket wait.
The limit covers one request, not a whole run of many clause assessments.
Requests, responses and source files have byte caps, not verified model-token accounting.
`response_format` is explicitly `json_schema` or `json_object`; no silent downgrade occurs.
Both modes receive the schema and undergo the same local validation.
Optional `temperature`, `max_tokens`, and `engagement` are explicit settings; engagement requires a bound credential.

The accepted Markdown subset is grouped paragraphs and single-line bullets with trailing `=group.variant` and hashtag metadata.
Baseline, JD, clause and approval inputs must be bounded regular files; named pipes and other special files are rejected before reading.
Multiline summary paragraphs are supported; headings/role/era context is retained.
Fences, blockquotes, list continuations, heading annotations, draft/verification metadata, duplicate IDs and groups without a `default` variant are rejected.
Ungrouped contact text, education and role descriptions remain context, not selectable evidence.
This is not a replacement for the renderer's full Markdown parser or validation.

After reviewing/correcting the clause file, create a private approval JSON containing `reviewer`, `jd_sha256` and `clauses_sha256`.
Hash the exact saved JD bytes and corrected clause-file bytes with SHA-256.
This is an explicit reviewer attestation, not a cryptographic identity signature.
Re-editing either file requires a new approval.
Do not let an agent create an approval on your behalf without your review decision.

```sh
python3 advise.py --config /absolute/private/gateway.toml \
  --baseline /absolute/private/Resume-Baseline.md --jd /absolute/private/jd.txt \
  --clauses /absolute/private/reviewed-clauses.json \
  --approval /absolute/private/approval.json --out /absolute/private/new-assessment
```

Clause rows include `source_start`, the exact zero-based character offset of `parent` in the JD.
This distinguishes identical text in different locations; duplicate checking is by occurrence, not global quote text.
Legacy files without offsets are accepted only when each parent occurs uniquely.
Quotes must occur exactly once within their parent, including overlapping matches.

A supplied clauses file has this shape; `quote` must occur within `parent`, and `parent` within the JD:

```json
{"clauses":[{"id":"r01","quote":"Manage engineering budgets","parent":"Manage engineering budgets and cloud spending.","importance":"required"}]}
```

Outputs live in an exclusively created mode-0700 directory outside this checkout.
They include frozen inputs, evidence IDs, clause and assessment requests, raw responses, validated proposals, model/usage metadata and code/input hashes.
No credential request headers are persisted.
Literal and JSON-equivalent token echoes are discarded before response persistence; this does not guarantee detection of arbitrary malicious encodings.
Private artifacts can contain all supplied personal data and provider-returned content: do not publish them.
The served model is recorded separately because gateway aliases may route to a concrete model.
A complete run has `status.json` set to `complete-unreviewed` plus `report.json`.
`status.json` is published by atomic replacement and starts as `running`.
A failed or interrupted run retains earlier diagnostics, is marked `failed` with only an allowlisted stage, category and HTTP status (never exception text or server content), and has no aggregate report.
If even that write fails, a `running` or missing status means the run is not complete.
`context-blocks.json` lists unannotated prose seen in the baseline; it is recorded for review and is not evidence.
Do not retry into an existing directory or mistake earlier successful stages for a complete run.

Every assessment validates its clause ID, existing evidence IDs, literal citation quotes and output shape.
Truncation, refusal and tool calls are rejected without repair.
Those checks prove reference integrity, not entailment, requirement coverage, or hiring suitability.
The report's citation union is not a shortlist: uncited evidence may still be relevant, and cited statements may overlap.
Both extraction coverage and semantic judgments require human review before downstream tailoring.

## Local-model evaluation against the frontier reference

`model_compare.py` freezes an owner-approved, complete gateway run as a fixture, reruns the same clauses on another model, and compares the two.
The frontier answers are a reference, not ground truth: an agreeing local answer is evidence, a disagreement needs reading.

```sh
python3 model_compare.py freeze /private/runs/approved-run /private/fixtures/name
python3 model_compare.py run /private/fixtures/name --config /private/local.toml --out /private/runs/local-v1
python3 model_compare.py compare /private/fixtures/name /private/runs/local-v1 --report /private/runs/local-v1/compare.json
```

`freeze` refuses a run that is incomplete or was not assessed from an approved clause file, and records a SHA-256 for every fixture file.
`run` and `compare` refuse a fixture that was edited after freezing.
A malformed or failed answer is recorded by category and the run continues, so one bad answer does not hide the others.
`compare` reports exact support agreement, clauses where the candidate claims more (the serious case) or less than the reference, failures, and mean citation overlap.
`compare` also lists answers that validation rejected but whose saved raw response claimed `direct` (`rejected_claiming_direct`), and on a probe fixture those that are overclaims on a negative, near-miss or trap.
It reads only the support label from a rejected answer; nothing is repaired and the answer is not counted as valid.
It does not check that the candidate's boundary text is correct; read those for the disagreements.
Fixtures and runs must live outside this checkout.

### Probe fixtures (false support)

The frontier fixtures contain no not-established answers, so they cannot show a model inventing support.
A probe fixture adds assistant-authored expectations in four categories:
- `negative`: nothing related in the baseline; must be not-established.
- `near-miss`: the key item is absent but related content exists; adjacent or not-established is acceptable, direct is the failure.
- `trap`: wording that inflates real evidence; adjacent or not-established, never direct.
- `control`: plainly supported; must be direct and cite the named wording.

The engine's own definition of adjacent is relevant but incomplete support, so a near-miss answered adjacent is correct when its boundary text says the item is not established.
Read the boundary of every adjacent answer on a near-miss: the label alone does not show whether the explanation claims the item.

```sh
python3 model_compare.py build-probe /private/probes-spec.json /private/Resume-Baseline.md /private/fixtures/probes-v1
python3 model_compare.py run /private/fixtures/probes-v1 --config /private/local.toml --out /private/runs/probes-local-v1
python3 model_compare.py compare /private/fixtures/probes-v1 /private/runs/probes-local-v1
```

`build-probe` refuses a negative or near-miss that has no `absent_terms`, or whose terms occur anywhere in the baseline, and refuses a control that cites an unknown wording ID.
Compare reports false support on negatives, `direct` on near-misses and traps, controls not judged direct, and controls judged direct without citing the expected wording.
The labels are not owner-reviewed and the frontier model has not been run against them.
Run the frontier config on the probes once before relying on them: a probe the frontier also fails is a label to reread.
Each probe sees the earlier probe texts as preceding context, the same way real clauses see earlier JD text.

## Tests

```sh
cd tools/advisor-spike
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -v
```

Tests use synthetic temporary files only.
Root Cargo gates do not exercise this standalone script.
Full spec evaluation, context-completeness validation, the second model, and owner usefulness go/no-go remain separate requirements of the broader spike (`rsm-eam.1`), not evidence-engine completion (`rsm-eam.3`).
