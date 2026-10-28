#!/usr/bin/env python3
"""Snapshot Arcade user docs with Python's standard library only.

Network mode reads stable GitHub release tags and catalog docs on main. Local
mode copies working-tree docs from sibling checkouts and labels them `local`;
it never misrepresents unpublished docs as a tagged release. Missing docs are
recorded. The 8 MB budget includes the manifest, and an existing output is
replaced only after the complete snapshot passes validation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import re
import sys
import tempfile
from urllib.error import HTTPError
from urllib.parse import quote, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

BUDGET = 8_000_000
CATALOG_URL = "https://raw.githubusercontent.com/qa-p1/Arcade-catalog/main/catalog.json"
CATALOG_REPO = "qa-p1/Arcade-catalog"
HOSTS = {"api.github.com", "raw.githubusercontent.com", "github.com"}
EXTENSIONS = {".md", ".txt", ".json", ".png", ".jpg", ".jpeg", ".webp", ".gif", ".svg", ".avif"}
IMAGES = {".png", ".jpg", ".jpeg", ".webp", ".gif", ".svg", ".avif"}
# Inline links and images, reference definitions, and HTML src/href.
LINKS = re.compile(r'(?P<pre>!?\[[^\]]*\]\()(?P<url><[^>]*>|[^)\s]+)|(?P<rpre>^\s*\[[^\]]+\]:\s*)(?P<rurl>\S+)|(?P<hpre>\b(?:src|href)=")(?P<hurl>[^"]+)', re.M)
EXTERNAL = "_external"


def external(target):
    return target.startswith(("#", "//")) or re.match(r"[a-z][a-z0-9+.-]*:", target, re.I) is not None


def links(text):
    """Every link target in a Markdown file, with its span."""
    for match in LINKS.finditer(text):
        for name in ("url", "rurl", "hurl"):
            if match.group(name):
                yield match.span(name), match.group(name).strip("<>")


def trusted_url(url):
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or parsed.hostname not in HOSTS
            or parsed.username or parsed.password or parsed.port not in (None, 443)):
        raise ValueError(f"Untrusted docs URL: {url}")


# CI passes its token: anonymous API calls share a small hourly limit per runner.
TOKEN = os.environ.get("GITHUB_TOKEN", "")


class TrustedRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        trusted_url(newurl)
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        # The token is for the GitHub API only, never a host it redirects to.
        if redirected is not None and urlsplit(newurl).hostname != "api.github.com":
            redirected.remove_header("Authorization")
        return redirected


OPENER = build_opener(TrustedRedirect())


def fetch(url, limit):
    trusted_url(url)
    headers = {"User-Agent": "Arcade-Tools-docs-snapshot/0.2.0", "Accept": "application/vnd.github+json"}
    if TOKEN and urlsplit(url).hostname == "api.github.com":
        headers["Authorization"] = f"Bearer {TOKEN}"
    request = Request(url, headers=headers)
    with OPENER.open(request, timeout=20) as response:
        content = response.read(limit + 1)
    if len(content) > limit:
        raise ValueError(f"Docs budget/metadata size limit exceeded by {url}")
    return content


def repo_allowed(repo):
    return re.fullmatch(r"qa-p1/[A-Za-z0-9_.-]+", repo or "") and repo.split("/")[1] not in (".", "..")


def safe_relative(path):
    if (not isinstance(path, str) or not path or "\\" in path or ":" in path
            or any(ord(c) < 32 for c in path) or path.startswith("/")):
        raise ValueError(f"Unsafe docs path: {path!r}")
    parts = path.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise ValueError(f"Unsafe docs path: {path!r}")
    return PurePosixPath(path)


def normalize(name):
    return re.sub(r"[^a-z0-9]", "", name.lower())


def unsafe_symlink(path):
    if not path.is_symlink():
        return False
    aliases = {Path('/var'): Path('/private/var'), Path('/tmp'): Path('/private/tmp')}
    return not (sys.platform == 'darwin' and path in aliases and path.resolve() == aliases[path])


def checkout(root, repo):
    wanted = normalize(repo.split("/")[1])
    candidates = [root]
    candidates.extend(sorted(p for p in root.iterdir() if p.is_dir() and not p.is_symlink()))
    for parent in list(candidates[1:]):
        candidates.extend(sorted(p for p in parent.iterdir() if p.is_dir() and not p.is_symlink()))
    exact = next((p for p in candidates if normalize(p.name) == wanted), None)
    if exact and (exact / ("docs" if wanted == "arcadecatalog" else "docs/user")).is_dir():
        return exact
    lanes = sorted((p for p in candidates if normalize(p.name.split("-wt-", 1)[0]) == wanted and (p / "docs/user").is_dir()), key=lambda p: ("-wt-docs" not in p.name, p.name))
    return lanes[0] if lanes else exact


class Snapshot:
    def __init__(self, stage, budget):
        self.stage = stage
        self.budget = budget
        self.size = 0
        self.entries = []

    def add_file(self, entry, relative, content):
        relative = safe_relative(relative)
        if len(entry["files"]) >= 4096:
            raise ValueError("Docs file-count limit exceeded")
        if self.size + len(content) > self.budget:
            raise ValueError(f"Docs budget exceeded ({self.size + len(content)} > {self.budget} bytes)")
        target = self.stage / entry["id"] / entry["version"] / str(relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
        entry["files"].append({"path": str(relative), "size": len(content),
                                "sha256": hashlib.sha256(content).hexdigest()})
        self.size += len(content)

    def keep_links_inside(self, entry, docs_path, read):
        """User guides link only inside docs/user or to absolute URLs (STANDARD
        §10): a link that leaves the folder becomes the file's GitHub URL on the
        app's main branch, and an image it shows is copied into the snapshot.
        `read(repo_path)` returns a repository file's bytes, or None."""
        root = self.stage / entry["id"] / entry["version"]
        for item in list(entry["files"]):
            if not item["path"].endswith(".md"):
                continue
            path = root / item["path"]
            text = path.read_bytes().decode("utf-8", "replace")
            folder = posixpath.dirname(item["path"])
            out, last = [], 0
            for (start, end), target in links(text):
                if external(target):
                    continue
                location, _, fragment = target.partition("#")
                location = location.split("?", 1)[0]
                if not location:
                    continue
                inside = posixpath.normpath(posixpath.join(folder, location))
                if not inside.startswith("../") and inside != "..":
                    continue
                repo_path = posixpath.normpath(posixpath.join(docs_path, inside))
                if repo_path.startswith(".."):
                    new = f"https://github.com/{entry['repo']}"
                elif PurePosixPath(repo_path).suffix.lower() in IMAGES and (content := read(repo_path)) is not None:
                    copy = f"{EXTERNAL}/{repo_path}"
                    if copy not in {f["path"] for f in entry["files"]}:
                        self.add_file(entry, copy, content)
                    new = posixpath.relpath(copy, folder or ".")
                else:
                    new = f"https://github.com/{entry['repo']}/blob/main/{repo_path}" + (f"#{fragment}" if fragment else "")
                out.append(text[last:start] + new)
                last = end
            if not out:
                continue
            content = ("".join(out) + text[last:]).encode("utf-8")
            self.size += len(content) - item["size"]
            if self.size > self.budget:
                raise ValueError("Docs budget exceeded")
            path.write_bytes(content)
            item.update(size=len(content), sha256=hashlib.sha256(content).hexdigest())

    def local(self, entry, repository, docs_path):
        if repository is None:
            entry["status"] = "no_checkout"
            return
        source = repository / docs_path
        if source.is_symlink() or any(p.is_symlink() for p in source.parents if p != repository and repository in p.parents):
            raise ValueError(f"Docs root must not be a symlink: {source}")
        if not source.is_dir():
            entry["status"] = "no_docs"
            return
        for directory, dirs, files in os.walk(source, followlinks=False):
            dirs.sort()
            for name in dirs:
                if (Path(directory) / name).is_symlink():
                    raise ValueError("Docs snapshot cannot contain symlinks")
            for name in sorted(files):
                path = Path(directory) / name
                if path.is_symlink():
                    raise ValueError("Docs snapshot cannot contain symlinks")
                if path.suffix.lower() not in EXTENSIONS:
                    continue
                if not path.is_file():
                    raise ValueError(f"Docs snapshot cannot contain special files: {path}")
                if path.stat().st_size > self.budget - self.size:
                    raise ValueError("Docs budget exceeded")
                with path.open("rb") as stream:
                    content = stream.read(self.budget - self.size + 1)
                self.add_file(entry, path.relative_to(source).as_posix(), content)
        entry["status"] = "snapshotted" if entry["files"] else "no_docs"

        def read(repo_path):
            file = repository / repo_path
            if file.is_symlink() or not file.is_file() or repository not in file.resolve().parents:
                return None
            return file.read_bytes() if file.stat().st_size <= self.budget - self.size else None
        self.keep_links_inside(entry, docs_path, read)

    def remote(self, entry, docs_path):
        pending = [docs_path]
        seen = set()
        while pending:
            path = pending.pop()
            safe_relative(path)
            if path in seen or len(seen) >= 4096:
                raise ValueError("Invalid or oversized docs directory tree")
            seen.add(path)
            url = f"https://api.github.com/repos/{entry['repo']}/contents/{quote(path, safe='/')}?ref={quote(entry['ref'], safe='')}"
            try:
                listing = json.loads(fetch(url, 1024 * 1024))
            except HTTPError as error:
                if error.code == 404 and path == docs_path:
                    entry["status"] = "no_docs"
                    return
                raise
            if not isinstance(listing, list):
                raise ValueError("Docs contents API did not return a directory")
            for item in listing:
                child = item["path"]
                safe_relative(child)
                if str(PurePosixPath(child).parent) != path:
                    raise ValueError("Docs contents API escaped the requested directory")
                if item["type"] == "dir":
                    pending.append(child)
                elif item["type"] == "file" and PurePosixPath(child).suffix.lower() in EXTENSIONS:
                    relative = str(PurePosixPath(child).relative_to(docs_path))
                    raw = f"https://raw.githubusercontent.com/{entry['repo']}/{quote(entry['ref'], safe='')}/{quote(child, safe='/')}"
                    self.add_file(entry, relative, fetch(raw, self.budget - self.size))
                elif item["type"] not in ("file",):
                    raise ValueError("Docs snapshot cannot contain symlinks/submodules")
        entry["status"] = "snapshotted" if entry["files"] else "no_docs"

        def read(repo_path):
            safe_relative(repo_path)
            raw = f"https://raw.githubusercontent.com/{entry['repo']}/{quote(entry['ref'], safe='')}/{quote(repo_path, safe='/')}"
            try:
                return fetch(raw, self.budget - self.size)
            except HTTPError:
                return None
        self.keep_links_inside(entry, docs_path, read)

    def finish(self):
        manifest = {"schema": 1, "contentBytes": self.size, "entries": self.entries}
        content = (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode()
        if self.size + len(content) > self.budget:
            raise ValueError("Docs budget exceeded by manifest")
        (self.stage / "manifest.json").write_bytes(content)
        return self.size + len(content)


def broken_links(root):
    """Relative links under `root` that don't reach a file inside it."""
    root = root.resolve()
    found = []
    for path in sorted(root.rglob("*.md")):
        text = path.read_text(encoding="utf-8", errors="replace")
        for _, target in links(text):
            if external(target):
                continue
            location = target.partition("#")[0].split("?", 1)[0]
            if not location:
                continue
            resolved = (path.parent / location).resolve()
            if root not in resolved.parents or not resolved.is_file():
                found.append(f"{path.relative_to(root)}: {target}")
    return found


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep-bundled", action="store_true", help="Keep a labeled bundled snapshot if a released tag has no docs")
    parser.add_argument("--from-local", type=Path, help="Root containing sibling app checkouts (no network)")
    parser.add_argument("--app-checkout", action="append", default=[], metavar="ID=PATH", help="Override one local app checkout, for integration worktrees")
    parser.add_argument("--catalog", type=Path, help="Explicit schema-1 catalog file")
    parser.add_argument("--out", type=Path, default=Path(__file__).resolve().parents[1] / "src-tauri/resources/docs")
    parser.add_argument("--budget-bytes", type=int, default=BUDGET, help="Lower test budget; cannot exceed 8 MB")
    parser.add_argument("--check-links", type=Path, nargs="+", metavar="DIR",
                        help="Only check: fail on any relative link that doesn't reach a file inside DIR")
    args = parser.parse_args(argv)
    if args.check_links:
        broken = [f"{folder}/{link}" for folder in args.check_links for link in broken_links(folder)]
        for link in broken:
            print(f"broken: {link}", file=sys.stderr)
        print(f"Checked links in {', '.join(map(str, args.check_links))}: {len(broken)} broken")
        return 1 if broken else 0
    if not 0 < args.budget_bytes <= BUDGET:
        parser.error("Budget must be positive and no greater than 8 MB")
    local = args.from_local.resolve() if args.from_local else None
    if local and not local.is_dir():
        parser.error("--from-local must be a directory")
    overrides = {}
    for override in args.app_checkout:
        app_id, separator, path = override.partition('=')
        if not local or not separator or not path or app_id in overrides:
            parser.error("--app-checkout needs --from-local and distinct ID=PATH values")
        overrides[app_id] = Path(path).resolve()
        if not overrides[app_id].is_dir():
            parser.error("App checkout must be a directory")
    catalog_checkout = checkout(local, CATALOG_REPO) if local else None
    catalog_path = args.catalog or (catalog_checkout / "catalog.json" if catalog_checkout else None)
    if local and catalog_path is None:
        catalog_path = Path(__file__).resolve().parents[1] / "resources/catalog-snapshot/catalog.json"
    catalog = json.loads(catalog_path.read_bytes() if catalog_path else fetch(CATALOG_URL, 1024 * 1024))
    if type(catalog.get("schema")) is not int or catalog["schema"] != 1:
        raise ValueError("Only catalog schema 1 is supported")
    out = args.out.absolute()
    if out.is_symlink() or any(unsafe_symlink(p) for p in out.parents):
        raise ValueError("Snapshot output cannot have symlinked parents")
    if out.exists() and any(out.iterdir()) and not (out / "manifest.json").is_file():
        raise ValueError("Existing output is not a docs snapshot; refusing to replace it")
    out.parent.mkdir(parents=True, exist_ok=True)
    ids = set()
    with tempfile.TemporaryDirectory(prefix=".docs-snapshot-", dir=out.parent) as temporary:
        stage = Path(temporary) / "docs"
        stage.mkdir()
        snapshot = Snapshot(stage, args.budget_bytes)
        entries = [(app["id"], app["repo"], "docs/user") for app in catalog["apps"]]
        entries.append(("arcade.ecosystem", CATALOG_REPO, "docs"))
        if catalog.get("tools", {}).get("repo"):
            entries.append(("arcade.tools", catalog["tools"]["repo"], "docs/user"))
        for app_id, repo, docs_path in entries:
            if not repo_allowed(repo):
                print(f"Dropped {app_id}: repository owner is not allowed", file=sys.stderr)
                continue
            if not re.fullmatch(r"arcade\.[a-z][a-z0-9-]*", app_id) or app_id in ids:
                raise ValueError("Invalid or duplicate docs app ID")
            ids.add(app_id)
            entry = {"id": app_id, "repo": repo, "version": "local" if local else "main",
                     "ref": "working-tree" if local else "main", "status": "no_docs", "files": []}
            snapshot.entries.append(entry)
            if local:
                snapshot.local(entry, overrides.get(app_id) or checkout(local, repo), docs_path)
            else:
                if app_id != "arcade.ecosystem":
                    try:
                        release = json.loads(fetch(f"https://api.github.com/repos/{repo}/releases/latest", 1024 * 1024))
                    except HTTPError as error:
                        if error.code == 404:
                            entry["status"] = "no_stable_release"
                            continue
                        raise
                    tag = release["tag_name"]
                    if release.get("draft") or release.get("prerelease") or not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:\+[0-9A-Za-z.-]+)?", tag):
                        raise ValueError("Latest stable release is not a stable version tag")
                    entry["version"], entry["ref"] = tag[1:], tag
                snapshot.remote(entry, docs_path)
        if args.keep_bundled and (out / "manifest.json").is_file():
            previous = json.loads((out / "manifest.json").read_bytes())
            for entry in snapshot.entries:
                if entry["files"]:
                    continue
                old = next((item for item in previous.get("entries", []) if item["id"] == entry["id"] and item.get("files")), None)
                if old:
                    safe_relative(old["version"])
                    entry.update({key: old[key] for key in ("version", "ref", "status")})
                    for item in old["files"]:
                        relative = safe_relative(item["path"])
                        source = out / entry["id"] / entry["version"] / str(relative)
                        if source.is_symlink() or any(unsafe_symlink(parent) for parent in source.parents):
                            raise ValueError("Bundled fallback cannot contain symlinks")
                        content = source.read_bytes()
                        if len(content) != item["size"] or hashlib.sha256(content).hexdigest() != item["sha256"]:
                            raise ValueError("Bundled fallback checksum mismatch")
                        snapshot.add_file(entry, str(relative), content)
        total = snapshot.finish()
        previous = Path(temporary) / "previous"
        if out.exists():
            out.rename(previous)
        try:
            stage.rename(out)
        except OSError:
            if previous.exists():
                previous.rename(out)
            raise
    print(f"Docs snapshot: {len(snapshot.entries)} entries, {total}/{args.budget_bytes} bytes -> {out}")
    for entry in snapshot.entries:
        print(f"  {entry['id']} {entry['version']}: {entry['status']} ({len(entry['files'])} files)")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError) as error:
        print(f"docs-snapshot: {error}", file=sys.stderr)
        sys.exit(1)
