"""Check that the stamped CONSENSUS_LOCK is the dependency graph Cargo actually resolves.

The list is hashed into every state root (SPEC.md §13). If it drifts from what is really
being compiled, the chain commits to versions it is not running — which is worse than not
stamping a list at all, because it looks like a guarantee.

The rule is per-edge rather than per-graph. A second copy of a crate elsewhere in the tree is
not by itself a problem: `age` pulls `sha2 0.11` through `rust-embed`, for its localized error
strings, while every cryptographic user of `sha2` resolves 0.10.9. What must hold is that no
consensus-relevant package depends on a version other than the stamped one.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

constants = (ROOT / "crates/popcorn-core/src/constants.rs").read_text()
block = re.search(r"CONSENSUS_LOCK: \[\(&str, &str\); (\d+)\] = \[(.*?)\];", constants, re.S)
if not block:
    sys.exit("CONSENSUS_LOCK is not declared in constants.rs")

declared_len = int(block.group(1))
stamped = dict(re.findall(r'\("([^"]+)",\s*"([^"]+)"\)', block.group(2)))
if len(stamped) != declared_len:
    sys.exit(f"CONSENSUS_LOCK declares {declared_len} entries but lists {len(stamped)}")

# Parse Cargo.lock into {name: {version: [dependency edges]}}.
packages: dict[str, dict[str, list[str]]] = {}
for raw in (ROOT / "Cargo.lock").read_text().split("[[package]]")[1:]:
    name = re.search(r'\nname = "([^"]+)"', raw)
    version = re.search(r'\nversion = "([^"]+)"', raw)
    if not name or not version:
        continue
    deps = re.search(r"dependencies = \[(.*?)\]", raw, re.S)
    edges = re.findall(r'"([^"]+)"', deps.group(1)) if deps else []
    packages.setdefault(name.group(1), {})[version.group(1)] = edges

# Anything that can move a state root: the stamped crates themselves, and our own.
consensus = set(stamped) | {"popcorn-core", "popcorn-timelock", "popcorn-node"}

problems = []
for crate, version in stamped.items():
    if crate not in packages:
        problems.append(f"{crate} is stamped at {version} but is absent from Cargo.lock")
    elif version not in packages[crate]:
        found = ", ".join(sorted(packages[crate]))
        problems.append(f"{crate} is stamped at {version} but Cargo.lock has {found}")

for package, versions in packages.items():
    if package not in consensus:
        continue
    for package_version, edges in versions.items():
        for edge in edges:
            parts = edge.split()
            dependency = parts[0]
            if dependency not in stamped:
                continue
            # An edge without a version is unambiguous: exactly one copy is in the graph.
            resolved = parts[1] if len(parts) > 1 else next(iter(packages[dependency]))
            if resolved != stamped[dependency]:
                problems.append(
                    f"{package} {package_version} depends on {dependency} {resolved}, "
                    f"but {dependency} is stamped at {stamped[dependency]}"
                )

if problems:
    for problem in sorted(set(problems)):
        print(f"  {problem}", file=sys.stderr)
    sys.exit(1)

print(f"{len(stamped)} stamped dependencies match every consensus edge in Cargo.lock")
