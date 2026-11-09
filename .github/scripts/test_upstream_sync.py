import tomllib
import unittest
from contextlib import chdir
from pathlib import Path
from tempfile import TemporaryDirectory

from upstream_sync import output, prepare_merge, release_version, run, set_version, synced_version


class UpstreamSyncTests(unittest.TestCase):
    def test_sync_preserves_the_custom_revision(self):
        self.assertEqual(synced_version("v0.4.8", "0.4.7+custom.6"), "0.4.8+custom.6")
        self.assertEqual(synced_version("v0.5.0", "0.4.9+custom.12"), "0.5.0+custom.12")
        with self.assertRaises(ValueError):
            synced_version("v0.4.8", "0.4.7")

    def test_only_new_stable_releases_are_proposed(self):
        current = "0.4.8+custom.7"
        self.assertEqual(release_version({"tag_name": "v0.4.10"}, current), "v0.4.10")
        for release in (
            {"tag_name": "v0.4.8"},
            {"tag_name": "v0.4.7"},
            {"tag_name": "v0.4.9-beta.1"},
            {"tag_name": "v0.4.9", "prerelease": True},
            {"tag_name": "v0.4.9", "draft": True},
        ):
            self.assertIsNone(release_version(release, current))

    def test_only_workspace_package_versions_are_rewritten(self):
        current = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
        for filename, expected in (("Cargo.toml", 1), ("Cargo.lock", 2)):
            with self.subTest(filename=filename):
                original = Path(filename).read_text(encoding="utf-8")
                changed = set_version(filename, original, "900.900.900+custom.1")
                self.assertEqual(changed.count('version = "900.900.900+custom.1"'), expected)
                self.assertEqual(set_version(filename, changed, current), original)

    def test_merge_resolves_version_only_conflicts_and_keeps_other_conflicts_for_review(self):
        for source_conflict in (False, True):
            with self.subTest(source_conflict=source_conflict), TemporaryDirectory() as directory, chdir(directory):
                run("git", "init", "-b", "custom")
                run("git", "config", "user.name", "Test")
                run("git", "config", "user.email", "test@example.invalid")
                manifest = Path("Cargo.toml")
                lockfile = Path("Cargo.lock")
                source = Path("example.txt")

                def commit(version, content):
                    manifest.write_text(f'[workspace.package]\nversion = "{version}"\n', encoding="utf-8")
                    lockfile.write_text(
                        f'[[package]]\nname = "ds-proto"\nversion = "{version}"\n\n'
                        f'[[package]]\nname = "dsz"\nversion = "{version}"\n', encoding="utf-8",
                    )
                    source.write_text(content, encoding="utf-8")
                    run("git", "add", ".")
                    run("git", "commit", "-m", version)

                commit("0.4.8", "original")
                run("git", "branch", "upstream")
                commit("0.4.8+custom.1", "fork")
                fork = output("git", "rev-parse", "HEAD")
                run("git", "switch", "upstream")
                commit("0.4.9", "upstream" if source_conflict else "original")
                upstream = output("git", "rev-parse", "HEAD")
                run("git", "switch", "custom")

                head, conflicts = prepare_merge(upstream, "0.4.9+custom.1")
                if source_conflict:
                    self.assertEqual((head, conflicts), (upstream, ["example.txt"]))
                    self.assertEqual(output("git", "rev-parse", "HEAD"), fork)
                else:
                    self.assertEqual((head, conflicts), ("HEAD", []))
                    self.assertEqual(output("git", "rev-parse", "HEAD^2"), upstream)
                    self.assertIn('version = "0.4.9+custom.1"', manifest.read_text(encoding="utf-8"))
                    self.assertEqual(source.read_text(encoding="utf-8"), "fork")
                self.assertEqual(output("git", "status", "--porcelain"), "")


if __name__ == "__main__":
    unittest.main()
