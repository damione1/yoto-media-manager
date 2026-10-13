"""Offline release regressions using disposable files and a separate test key."""

import base64
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
import zipfile
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("updater_release", ROOT / "scripts/updater_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseVersionTests(unittest.TestCase):
    def test_only_matching_stable_version_tags_are_accepted(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        for tag in ["v" + version, "master", "v999.0.0", "v01.0.0", "v1.2.3-beta.1", "v1.2.3+build"]:
            with self.subTest(tag=tag):
                result = subprocess.run(
                    ["python3", str(ROOT / "scripts/release_version.py")],
                    env={**os.environ, "GITHUB_REF_NAME": tag}, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode == 0, tag == "v" + version)
                if result.returncode == 0:
                    self.assertEqual(result.stdout, "version=" + version + "\n")


class UpdaterReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="yoto-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        self.version = "1.2.3"
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.3"\n')
        (self.root / "distribution").mkdir()
        (self.root / "distribution/THIRD-PARTY-NOTICES.md").write_text("Test notices\n")
        self.patch = mock.patch.object(release, "ROOT", self.root)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        for target, (platform, extension) in release.PLATFORMS.items():
            if extension == "tar.gz":
                app = self.root / "dist" / target / "Yoto.app"
                (app / "Contents/MacOS").mkdir(parents=True)
                (app / "Contents/MacOS/yoto-desktop").write_bytes(b"test desktop")
                (app / "Contents/Resources").mkdir()
                (app / "Contents/Resources/current").symlink_to("../MacOS/yoto-desktop")
            else:
                binaries = self.root / "target" / target / "release"
                binaries.mkdir(parents=True)
                for name in ["yoto-desktop.exe", "yoto-mcp.exe"]:
                    (binaries / name).write_bytes(name.encode())
                tools = self.root / "tools" / target
                (tools / "nested").mkdir(parents=True)
                (tools / "ffmpeg.exe").write_bytes(b"test tool")
                (tools / "nested/manifest.json").write_text("{}")

    def fake_minisign(self, args, **kwargs):
        if "-S" in args:
            key = pathlib.Path(args[args.index("-s") + 1])
            self.assertEqual(key.stat().st_mode & 0o777, 0o600)
            self.assertEqual(key.read_bytes(), b"disposable-test-key")
            signature = pathlib.Path(args[args.index("-x") + 1])
            trusted = args[args.index("-t") + 1]
            signature.write_text("untrusted comment: test\nTEST\ntrusted comment: " + trusted + "\nTEST\n")
        return subprocess.CompletedProcess(args, 0)

    def build_all(self):
        for target in release.PLATFORMS:
            release.build(target)
        folder = self.root / "release"
        folder.mkdir()
        for target in release.PLATFORMS:
            for path in (self.root / "dist" / target).iterdir():
                if path.is_file():
                    shutil.copy2(path, folder / path.name)
        return folder

    def test_archives_keep_expected_layout_and_mac_symlinks(self):
        with mock.patch.dict(os.environ, {"YOTO_UPDATER_PRIVATE_KEY": base64.b64encode(b"disposable-test-key").decode()}), mock.patch.object(release.subprocess, "run", side_effect=self.fake_minisign):
            folder = self.build_all()
            release.assemble(folder)
        manifest = json.loads((folder / "latest.json").read_text())
        self.assertEqual(manifest["version"], self.version)
        self.assertEqual(set(manifest["platforms"]), {pair[0] for pair in release.PLATFORMS.values()})
        for platform, extension in release.PLATFORMS.values():
            archive = folder / f"Yoto-{self.version}-{platform}.{extension}"
            self.assertEqual(manifest["platforms"][platform]["size"], archive.stat().st_size)
            if extension == "tar.gz":
                with tarfile.open(archive) as reader:
                    link = reader.getmember("Yoto.app/Contents/Resources/current")
                    self.assertTrue(link.issym())
                    self.assertEqual(link.linkname, "../MacOS/yoto-desktop")
            else:
                with zipfile.ZipFile(archive) as reader:
                    self.assertEqual(set(reader.namelist()), {
                        "Yoto/yoto-desktop.exe", "Yoto/yoto-mcp.exe", "Yoto/tools/ffmpeg.exe",
                        "Yoto/tools/nested/manifest.json", "Yoto/THIRD-PARTY-NOTICES.md",
                    })
        self.assertFalse(list(self.root.rglob("private.key")))

    def test_incomplete_release_and_metadata_mismatch_are_rejected(self):
        with mock.patch.dict(os.environ, {"YOTO_UPDATER_PRIVATE_KEY": base64.b64encode(b"disposable-test-key").decode()}), mock.patch.object(release.subprocess, "run", side_effect=self.fake_minisign):
            folder = self.build_all()
            path = folder / "windows-x86_64.json"
            original = path.read_text()
            path.unlink()
            with self.assertRaises(FileNotFoundError):
                release.assemble(folder)
            self.assertFalse((folder / "latest.json").exists())
            for field, value in [("version", "9.0.0"), ("platform", "darwin-aarch64"), ("url", "https://untrusted.example/archive.zip"), ("size", 1), ("signature", "invalid")]:
                with self.subTest(field=field):
                    metadata = json.loads(original)
                    metadata[field] = value
                    path.write_text(json.dumps(metadata))
                    with self.assertRaises(SystemExit):
                        release.assemble(folder)
                    self.assertFalse((folder / "latest.json").exists())

    @unittest.skipUnless(shutil.which("minisign"), "minisign is needed for the independent signing integration test")
    def test_real_signatures_require_matching_bytes_and_release_identity(self):
        private = self.root / "test-private.key"
        public = self.root / "distribution/updater.pub"
        subprocess.run(["minisign", "-G", "-W", "-s", str(private), "-p", str(public)], check=True, capture_output=True)
        key = base64.b64encode(private.read_bytes()).decode()
        original_run = subprocess.run

        def quiet_run(args, **kwargs):
            return original_run(args, **{**kwargs, "capture_output": True})

        with mock.patch.dict(os.environ, {"YOTO_UPDATER_PRIVATE_KEY": key}), mock.patch.object(release.subprocess, "run", side_effect=quiet_run):
            folder = self.build_all()
            release.assemble(folder)
            archive = folder / "Yoto-1.2.3-windows-x86_64.zip"
            original = archive.read_bytes()
            archive.write_bytes(bytes([original[0] ^ 1]) + original[1:])
            with self.assertRaises(subprocess.CalledProcessError):
                release.assemble(folder)
            archive.write_bytes(original)
            signature = archive.with_name(archive.name + ".minisig")
            quiet_run(["minisign", "-S", "-s", str(private), "-m", str(archive), "-x", str(signature), "-t", "YotoMediaManager Yoto-1.0.0-windows-x86_64.zip"], check=True)
            metadata_path = folder / "windows-x86_64.json"
            metadata = json.loads(metadata_path.read_text())
            metadata["signature"] = base64.b64encode(signature.read_bytes()).decode()
            metadata_path.write_text(json.dumps(metadata))
            with self.assertRaisesRegex(SystemExit, "Mismatched signed release identity"):
                release.assemble(folder)


if __name__ == "__main__":
    unittest.main()
