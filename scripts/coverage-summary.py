"""Summarise an LCOV report as line coverage of product code, per crate.

Each file's trailing `#[cfg(test)] mod` is excluded: it is test code, it always
runs, and counting it would report tests covering themselves.
"""
import collections
import pathlib
import re
import sys

TEST_MODULE = re.compile(r"\s*(pub\s+)?mod\s+\w+\s*\{")


def test_module_start(path: pathlib.Path) -> int:
    """First line of the file's trailing test module, or past the end."""
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return 10**9
    for index, line in enumerate(lines):
        if line.strip() != "#[cfg(test)]" or line.startswith(" "):
            continue
        following = next((l for l in lines[index + 1:] if l.strip()), "")
        if TEST_MODULE.match(following):
            return index + 1  # 1-based
    return 10**9


def is_test_support(crate: str) -> bool:
    """Crates marked `publish = false` exist to support tests, not to ship."""
    manifest = pathlib.Path("crates") / crate / "Cargo.toml"
    try:
        text = manifest.read_text()
    except OSError:
        return False
    return any(line.replace(" ", "") == "publish=false" for line in text.splitlines())


def main(report: str) -> None:
    root = pathlib.Path.cwd()
    crates = collections.defaultdict(lambda: [0, 0])
    files = []

    current = None
    cutoff = 0
    covered = total = 0

    for line in pathlib.Path(report).read_text().splitlines():
        if line.startswith("SF:"):
            current = pathlib.Path(line[3:])
            cutoff = test_module_start(current)
            covered = total = 0
        elif line.startswith("DA:") and current is not None:
            number, hits = line[3:].split(",")[:2]
            if int(number) >= cutoff:
                continue
            total += 1
            covered += int(hits) > 0
        elif line == "end_of_record" and current is not None:
            try:
                relative = current.relative_to(root)
            except ValueError:
                current = None
                continue
            parts = relative.parts
            if len(parts) > 2 and parts[0] == "crates" and total:
                crates[parts[1]][0] += covered
                crates[parts[1]][1] += total
                files.append((covered / total, covered, total, relative))
            current = None

    order = [
        "tailscale-localapi",
        "caddy-admin",
        "beszel-client",
        "cosmic-tailscale",
        "cosmic-applet-tailscale",
    ]
    names = [n for n in order if n in crates] + sorted(set(crates) - set(order))

    print(f"\n{'crate':<26}{'lines':>15}{'coverage':>10}")
    all_covered = all_total = 0
    support = []
    for name in names:
        if is_test_support(name):
            support.append(name)
            continue
        c, t = crates[name]
        all_covered += c
        all_total += t
        print(f"{name:<26}{c:>7}/{t:<7}{100 * c / t:>9.1f}%")
    if all_total:
        print(f"{'total (product code)':<26}{all_covered:>7}/{all_total:<7}{100 * all_covered / all_total:>9.1f}%")
    for name in support:
        c, t = crates[name]
        print(f"{name + ' (test support)':<26}{c:>7}/{t:<7}{100 * c / t:>9.1f}%  not counted")

    print("\nleast covered files (20+ lines):")
    substantial = [
        f for f in sorted(files)
        if f[2] >= 20 and not is_test_support(f[3].parts[1])
    ]
    for share, c, t, path in substantial[:15]:
        print(f"  {100 * share:5.1f}%  {c:>4}/{t:<4}  {path}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "target/coverage/lcov.info")
