"""Open one review PR per published upstream DSZ version."""

import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

UPSTREAM = "MaximumFreak/dsz"
BASE = "custom"
VERSION_FILES = ("Cargo.toml", "Cargo.lock")
STABLE_TAG = re.compile(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", re.ASCII)


def run(*args, check=True):
    return subprocess.run(args, check=check, text=True, stdout=subprocess.PIPE)


def output(*args):
    return run(*args).stdout.strip()


def release_version(release, current):
    match = STABLE_TAG.fullmatch(release.get("tag_name", ""))
    if not match or release.get("draft") or release.get("prerelease"):
        return None
    published = tuple(map(int, match.groups()))
    installed = tuple(map(int, current.split("+", 1)[0].split(".")))
    return match[0] if published > installed else None


def set_version(path, contents, version):
    if path == "Cargo.toml":
        headers = (r"\[workspace\.package\]",)
    else:
        headers = tuple(r'\[\[package\]\]\nname = "' + name + '"' for name in ("ds-proto", "dsz"))
    for header in headers:
        pattern = rf'(^({header})\n(?:(?!\[)[^\n]*\n)*?version = ")[^"\n]+(")'
        contents, count = re.subn(pattern, lambda match: match[1] + version + match[3], contents, flags=re.M)
        if count != 1:
            raise ValueError(f"Could not identify the version field in {path}: {header}")
    return contents


def prepare_merge(upstream, version):
    result = run("git", "merge", "--no-ff", "--no-commit", upstream, check=False)
    conflicts = output("git", "diff", "--name-only", "--diff-filter=U").splitlines()
    if result.returncode and not conflicts:
        raise RuntimeError("Git failed before reaching a merge conflict.")

    resolved = {}
    for path in conflicts:
        if path not in VERSION_FILES:
            continue
        stages = [run("git", "show", f":{stage}:{path}", check=False) for stage in (1, 2, 3)]
        if any(stage.returncode for stage in stages):
            continue
        ancestor, ours, theirs = (stage.stdout for stage in stages)
        # Keep fork edits only when upstream changed nothing except version fields.
        try:
            if set_version(path, ancestor, "0.0.0") == set_version(path, theirs, "0.0.0"):
                resolved[path] = set_version(path, ours, version)
        except ValueError:
            continue

    remaining = sorted(set(conflicts) - resolved.keys())
    if remaining:
        run("git", "merge", "--abort")
        return upstream, remaining

    for path in VERSION_FILES:
        if path in resolved:
            contents = resolved[path]
        else:
            contents = set_version(path, Path(path).read_text(encoding="utf-8"), version)
        Path(path).write_text(contents, encoding="utf-8", newline="\n")
    run("git", "add", "--", *VERSION_FILES)
    run("git", "diff", "--cached", "--check")
    run("git", "commit", "-m", f"Merge upstream DSZ {version.split('+', 1)[0]}")
    return "HEAD", []


def main():
    repo = os.environ["GITHUB_REPOSITORY"]
    release = json.loads(output("gh", "api", f"repos/{UPSTREAM}/releases/latest"))
    current = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    tag = release_version(release, current)
    if tag is None:
        print(f"No newer stable upstream release for {current}.")
        return

    branch = f"sync/upstream-{tag}"
    existing = json.loads(output("gh", "pr", "list", "--repo", repo, "--base", BASE,
                                 "--head", branch, "--state", "all", "--json", "url"))
    if existing:
        print(f"{tag} already has a review PR: {existing[0]['url']}")
        return

    ref = f"refs/heads/{branch}"
    remote_branch = output("git", "ls-remote", "--heads", "origin", ref)
    release_ref = f"refs/tags/{tag}"
    run("git", "fetch", "--no-tags", f"https://github.com/{UPSTREAM}.git", release_ref)
    upstream = output("git", "rev-parse", "FETCH_HEAD^{commit}")
    if run("git", "merge-base", "--is-ancestor", upstream, "HEAD", check=False).returncode == 0:
        print(f"{tag} is already integrated.")
        return
    if remote_branch:
        # A previous attempt can push successfully before PR creation fails.
        run("git", "fetch", "--no-tags", "origin", ref)
        run("git", "merge-base", "--is-ancestor", upstream, "FETCH_HEAD")
        conflicts = ["See GitHub's merge status"] if output("git", "rev-parse", "FETCH_HEAD") == upstream else []
    else:
        run("git", "config", "user.name", "github-actions[bot]")
        run("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
        head, conflicts = prepare_merge(upstream, f"{tag[1:]}+custom.1")
        run("git", "push", "origin", f"{head}:{ref}")

    body = f"Integrate [upstream DSZ {tag}](https://github.com/{UPSTREAM}/releases/tag/{tag}).\n\n"
    if conflicts:
        body += "Merge conflicts need review: " + ", ".join(conflicts) + ".\n\n"
    body += "Review the merge and run Windows checks before publishing a custom release."
    print(output("gh", "pr", "create", "--repo", repo, "--base", BASE, "--head", branch,
                 "--title", f"Sync upstream DSZ {tag}", "--body", body))


if __name__ == "__main__":
    main()
