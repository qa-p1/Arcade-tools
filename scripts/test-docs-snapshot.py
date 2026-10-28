#!/usr/bin/env python3
"""Deterministic stdlib-only tests; no requests leave this process."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

SPEC = importlib.util.spec_from_file_location("docs_snapshot", Path(__file__).with_name("docs-snapshot.py"))
SNAPSHOT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SNAPSHOT)
CATALOG = json.loads((Path(__file__).resolve().parents[1] / "resources/catalog-snapshot/catalog.json").read_bytes())


class SnapshotTests(unittest.TestCase):
    def run_snapshot(self, args):
        with contextlib.redirect_stdout(io.StringIO()):
            return SNAPSHOT.main(args)

    def test_links_leaving_the_guide_become_urls_and_images_are_copied(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            catalog = root / "Arcade-catalog"
            (catalog / "docs").mkdir(parents=True)
            (catalog / "catalog.json").write_text(json.dumps(CATALOG))
            (catalog / "docs/index.md").write_text("# Arcade")
            repo = root / "Arcade-look"
            (repo / "docs/user").mkdir(parents=True)
            (repo / "docs/hero.webp").write_bytes(b"webp")
            (repo / "docs/user/index.md").write_text(
                "![Hero](../hero.webp)\nSee [status](../STATUS.md#now), [FAQ](faq.md) and [site](https://example.com).\n"
                '<img src="../hero.webp" alt="">\n[ref]: ../PLUGINS.md\n')
            (repo / "docs/user/faq.md").write_text("# FAQ")
            out = root / "output"
            self.assertEqual(self.run_snapshot(["--from-local", str(root), "--out", str(out)]), 0)
            page = (out / "arcade.look/local/index.md").read_text()
            repo_url = "https://github.com/" + next(app["repo"] for app in CATALOG["apps"] if app["id"] == "arcade.look")
            self.assertIn("![Hero](_external/docs/hero.webp)", page)
            self.assertIn('<img src="_external/docs/hero.webp"', page)
            self.assertIn(f"[status]({repo_url}/blob/main/docs/STATUS.md#now)", page)
            self.assertIn(f"[ref]: {repo_url}/blob/main/docs/PLUGINS.md", page)
            self.assertIn("[FAQ](faq.md)", page)
            self.assertIn("(https://example.com)", page)
            self.assertEqual((out / "arcade.look/local/_external/docs/hero.webp").read_bytes(), b"webp")
            manifest = json.loads((out / "manifest.json").read_bytes())
            look = next(entry for entry in manifest["entries"] if entry["id"] == "arcade.look")
            index = next(item for item in look["files"] if item["path"] == "index.md")
            self.assertEqual(index["size"], len(page.encode()))
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(SNAPSHOT.main(["--check-links", str(out)]), 0)
                (out / "arcade.look/local/faq.md").write_text("[gone](../elsewhere.md)")
                self.assertEqual(SNAPSHOT.main(["--check-links", str(out)]), 1)

    def test_local_text_images_missing_docs_and_atomic_budget(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            catalog = root / "Arcade-catalog"
            (catalog / "docs").mkdir(parents=True)
            (catalog / "catalog.json").write_text(json.dumps(CATALOG))
            (catalog / "docs/index.md").write_text("# Arcade\nEcosystem docs")
            app = root / "Arcade-look/docs/user"
            (app / "images").mkdir(parents=True)
            (app / "index.md").write_text("# Preview\nUse Space.")
            (app / "images/preview.png").write_bytes(b"fake image")
            out = root / "output"
            args = ["--from-local", str(root), "--out", str(out)]
            self.assertEqual(self.run_snapshot(args), 0)
            manifest = json.loads((out / "manifest.json").read_bytes())
            entries = {entry["id"]: entry for entry in manifest["entries"]}
            self.assertEqual(entries["arcade.look"]["status"], "snapshotted")
            self.assertEqual(entries["arcade.look"]["version"], "local")
            self.assertEqual(entries["arcade.look"]["ref"], "working-tree")
            self.assertEqual(entries["arcade.box"]["status"], "no_checkout")
            self.assertEqual((out / "arcade.look/local/images/preview.png").read_bytes(), b"fake image")
            self.assertEqual(entries["arcade.ecosystem"]["status"], "snapshotted")
            previous = (out / "manifest.json").read_bytes()
            (app / "images/large.png").write_bytes(b"x" * 6000)
            with self.assertRaisesRegex(ValueError, "budget"):
                self.run_snapshot(args + ["--budget-bytes", "5000"])
            self.assertEqual((out / "manifest.json").read_bytes(), previous)

    def test_remote_uses_stable_tags_and_records_absent_docs(self):
        calls = []

        def fetch(url, limit):
            calls.append(url)
            if url == SNAPSHOT.CATALOG_URL:
                content = json.dumps(CATALOG).encode()
            elif "/Arcade-Find/releases/latest" in url:
                raise HTTPError(url, 404, "missing release", {}, None)
            elif "/releases/latest" in url:
                content = b'{"tag_name":"v1.2.3","draft":false,"prerelease":false}'
            elif "/Arcade-box/contents/docs/user" in url:
                raise HTTPError(url, 404, "missing docs", {}, None)
            elif "/contents/docs/user" in url:
                self.assertIn("ref=v1.2.3", url)
                content = b'[{"type":"file","path":"docs/user/index.md","size":12}]'
            elif "/contents/docs?ref=main" in url:
                content = b'[{"type":"file","path":"docs/index.md","size":12}]'
            elif "raw.githubusercontent.com" in url:
                content = b"# Stable docs"
            else:
                raise AssertionError(url)
            if len(content) > limit:
                raise ValueError("Docs budget exceeded")
            return content

        with tempfile.TemporaryDirectory() as temporary, patch.object(SNAPSHOT, "fetch", fetch):
            out = Path(temporary) / "docs"
            self.run_snapshot(["--out", str(out)])
            entries = {entry["id"]: entry for entry in json.loads((out / "manifest.json").read_bytes())["entries"]}
            self.assertEqual(entries["arcade.box"]["status"], "no_docs")
            self.assertEqual(entries["arcade.find"]["status"], "no_stable_release")
            self.assertEqual(entries["arcade.look"]["version"], "1.2.3")
            self.assertTrue(any("/v1.2.3/docs/user/index.md" in url for url in calls))

    def test_symlinks_and_unsafe_urls_paths_are_rejected(self):
        for url in ["http://api.github.com/x", "https://evil.example/x", "https://api.github.com.evil/x", "https://user@api.github.com/x"]:
            with self.assertRaises(ValueError):
                SNAPSHOT.trusted_url(url)
        for path in ["../escape", "/absolute", "docs/../escape", "docs\\escape", "docs//file"]:
            with self.assertRaises(ValueError):
                SNAPSHOT.safe_relative(path)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "repo/docs/user").mkdir(parents=True)
            try:
                (root / "repo/docs/user/escape").symlink_to(root)
            except OSError:
                return  # Windows without symlink privileges: logic tests still run.
            snapshot = SNAPSHOT.Snapshot(root / "stage", 1024)
            with self.assertRaisesRegex(ValueError, "symlink"):
                snapshot.local({"id":"arcade.look", "version":"local", "files":[]}, root / "repo", "docs/user")

    def test_explicit_worktree_override_includes_its_images(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            docs = root / 'Arcade-tools-wt-hub/docs/user'
            (docs / 'images').mkdir(parents=True)
            (docs / 'index.md').write_text('![Home](images/home.png)', encoding='utf-8')
            (docs / 'images/home.png').write_bytes(b'home')
            out = root / 'snapshot'
            self.run_snapshot(['--from-local', str(root), '--app-checkout', f'arcade.tools={docs.parents[1]}', '--out', str(out)])
            self.assertEqual((out / 'arcade.tools/local/images/home.png').read_bytes(), b'home')


if __name__ == "__main__":
    unittest.main()
