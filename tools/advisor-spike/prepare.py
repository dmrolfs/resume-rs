"""Prepare a review candidate from an explicit, private source-line recipe."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re

NAME = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")
REPO = Path(__file__).resolve().parents[2]


def private_path(path):
    resolved = Path(path).expanduser().resolve()
    if resolved == REPO or REPO in resolved.parents:
        raise ValueError("Private artifacts cannot be written inside the code repository")
    return resolved


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_new(path, content):
    import os
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(content if isinstance(content, bytes) else content.encode("utf-8"))


def load_sources(recipe):
    sources = {}
    for name, source in recipe["sources"].items():
        path = Path(source["path"]).expanduser().resolve()
        content = path.read_bytes()
        if hashlib.sha256(content).hexdigest() != source["sha256"]:
            raise ValueError(f"Source hash changed: {path}")
        sources[name] = (path, content.decode("utf-8").splitlines())
    return sources


def validate_nodes(nodes):
    groups = {}
    last_group = None
    for node in nodes:
        if not node["sources"]:
            raise ValueError("Each node requires source evidence")
        for name, line in node["sources"]:
            if not isinstance(name, str) or type(line) is not int or line < 1:
                raise ValueError("Source references require a name and positive integer line")
        group, variant = node["group"], node["variant"]
        if bool(group) != bool(variant):
            raise ValueError("Group and variant must be supplied together")
        for name in [group, variant, *node["tags"]]:
            if name is not None and not NAME.fullmatch(name):
                raise ValueError(f"Invalid annotation name: {name!r}")
        if group:
            if group in groups and last_group != group:
                raise ValueError(f"Nonadjacent group: {group}")
            variants = groups.setdefault(group, set())
            if variant in variants:
                raise ValueError(f"Duplicate variant: {group}.{variant}")
            variants.add(variant)
        last_group = group
    for group, variants in groups.items():
        if "default" not in variants:
            raise ValueError(f"Missing default: {group}")


def render_node(node, sources):
    texts = list(dict.fromkeys(sources[name][1][line - 1] for name, line in node["sources"]))
    if len(texts) > 1 and not all(text.startswith("- ") for text in texts):
        raise ValueError("Only whole bullet items can be merged")
    text = texts[0] + "".join(" " + t[2:] for t in texts[1:])
    tokens = []
    if node["group"]:
        tokens.append(f"={node['group']}.{node['variant']}")
    tokens.extend("#" + tag for tag in node["tags"])
    if node["verify"]:
        tokens.append("@verify")
    return text + (" <!-- " + " ".join(tokens) + " -->" if tokens else "")


def prepare(recipe_path, output):
    output = private_path(output)
    recipe = json.loads(Path(recipe_path).read_text())
    sources = load_sources(recipe)
    validate_nodes(recipe["nodes"])
    used = Counter(tuple(ref) for node in recipe["nodes"] for ref in node["sources"])
    used.update(tuple(item["source"]) for item in recipe["omissions"])
    expected = Counter((name, i) for name, (_, lines) in sources.items()
                       for i, line in enumerate(lines, 1) if line.strip())
    if used != expected:
        raise ValueError("Source coverage must account for each nonblank line exactly once")
    baseline = ["<!-- Candidate only: owner review pending; originals remain authoritative. -->", ""]
    mapping = []
    for node in recipe["nodes"]:
        text = render_node(node, sources)
        baseline_line = len(baseline) + 1
        baseline.extend([text, ""])
        for name, line in node["sources"]:
            mapping.append(source_row(name, line, sources, recipe,
                                      baseline_line, node, "merged" if len(node["sources"]) > 1 else "kept"))
    for omission in recipe["omissions"]:
        name, line = omission["source"]
        row = source_row(name, line, sources, recipe, None, None, "omitted-proposed")
        row["reason"] = omission["reason"]
        mapping.append(row)
    for name, (path, _) in sources.items():
        if digest(path) != recipe["sources"][name]["sha256"]:
            raise ValueError(f"Source hash changed during preparation: {path}")
    output.mkdir(mode=0o700)
    write_new(output / "Resume-Baseline.md", "\n".join(baseline))
    for name, data in (("source-map.json", mapping), ("decisions.json", recipe["decisions"]),
                       ("recipe.json", recipe)):
        write_new(output / name, json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    return {"source_lines": len(mapping), "baseline_nodes": len(recipe["nodes"]),
            "owner_approved": False, "output": str(output)}


def source_row(name, line, sources, recipe, baseline_line, node, disposition):
    return {"evidence_id": f"{name}-{line:04d}", "source_file": str(sources[name][0]),
            "source_sha256": recipe["sources"][name]["sha256"], "line": line,
            "source_text": sources[name][1][line - 1], "baseline_line": baseline_line,
            "group": node["group"] if node else None, "variant": node["variant"] if node else None,
            "disposition": disposition, "review_status": "pending"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recipe", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(prepare(args.recipe, args.output), indent=2))
