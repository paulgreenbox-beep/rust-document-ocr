"""Source-only tests; no runtime, network, Cargo or user documents."""
import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest import mock

spec = importlib.util.spec_from_file_location("inventory", Path(__file__).with_name("collect-portable-inventory.py"))
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class InventoryTests(unittest.TestCase):
    def test_standard_dual_license_notice_names_are_collected(self):
        for name in ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE.txt", "COPYRIGHT", "COPYING.LESSER"]:
            self.assertIsNotNone(inventory.LICENSE_NAMES.fullmatch(name), name)
        self.assertIsNone(inventory.LICENSE_NAMES.fullmatch("README.md"))

    def test_final_registry_record_without_newline_is_not_lost(self):
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "Cargo.lock"
            lock.write_text('version = 4\n[[package]]\nname="local"\nversion="1"\n[[package]]\nname="first"\nversion="1"\nsource="registry+https://example.invalid"\nchecksum="' + 'a' * 64 + '"\n[[package]]\nname="last"\nversion="2"\nsource="registry+https://example.invalid"\nchecksum="' + 'b' * 64 + '"', encoding="utf-8")
            records = inventory.parse_lock(lock)
            self.assertEqual([row["name"] for row in records], ["first", "last"])
            self.assertEqual(records[-1]["checksum"], "b" * 64)

    def test_repository_inventory_includes_last_locked_crate(self):
        records = inventory.parse_lock(Path(__file__).resolve().parents[1] / "Cargo.lock")
        self.assertEqual(len(records), 282)
        self.assertEqual(records[-1]["name"], "zune-jpeg")


    def test_vcpkg_status_is_per_port_and_missing_status_never_borrows_another(self):
        # Reproduces the old collector assigning the final .list package's
        # Version/Abi to every separately linked vcpkg archive.
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "vcpkg"
            installed = root / "installed"
            triplet = "x64-windows-static-md"
            info = installed / "vcpkg" / "info"
            libdir = installed / triplet / "lib"
            info.mkdir(parents=True)
            libdir.mkdir(parents=True)

            packages = [
                ("libheif", "1.23.1", "heif", "a" * 40, None),
                ("libde265", "1.1.1", "libde265", "b" * 40, "2"),
                ("zstd", "1.5.7", "zstd", "c" * 40, None),
                ("unknownport", "0.3.0", "unknownlib", None, None),
            ]
            statuses = []
            link_requests = []
            for port, version, lib, abi, port_version in packages:
                (info / f"{port}_{version}_{triplet}.list").write_text(
                    f"{triplet}/lib/{lib}.lib\n", encoding="utf-8"
                )
                (libdir / f"{lib}.lib").write_bytes(f"synthetic-{port}".encode("ascii"))
                link_requests.append(f"cargo:rustc-link-lib=static={lib}")
                if abi is not None:
                    lines = [
                        f"Package: {port}",
                        f"Version: {version}",
                        f"Architecture: {triplet}",
                        f"Abi: {abi}",
                    ]
                    if port_version is not None:
                        lines.append(f"Port-Version: {port_version}")
                    statuses.append("\n".join(lines))
            (installed / "vcpkg" / "status").write_text(
                "\n\n".join(statuses) + "\n", encoding="utf-8"
            )

            repo = base / "repo"
            output = repo / "target" / "release" / "build" / "synthetic" / "output"
            output.parent.mkdir(parents=True)
            output.write_text("\n".join(link_requests) + "\n", encoding="utf-8")
            with mock.patch.object(inventory, "REPO", repo):
                components, warnings = inventory.vcpkg_inventory(
                    root, base / "unused-output", base / "notices"
                )

            self.assertEqual(warnings, [])
            by_id = {component["id"]: component for component in components}
            self.assertEqual(set(by_id), {f"vcpkg:{port}" for port, *_ in packages})
            for port, version, _, abi, port_version in packages:
                component = by_id[f"vcpkg:{port}"]
                self.assertEqual(component["distribution_status"], "blocked")
                evidence = component["evidence"]["installed_status"]
                if abi is None:
                    # Without an installed-status stanza, no version or ABI
                    # may be invented or inherited from another package.
                    self.assertEqual(evidence, {})
                    self.assertEqual(component["version"], version)
                    continue
                self.assertEqual(evidence["Version"], version)
                self.assertEqual(evidence["Abi"], abi)
                self.assertEqual(evidence["Architecture"], triplet)
                self.assertEqual(evidence.get("Port-Version"), port_version)
                expected = version + (f"#{port_version}" if port_version else "")
                self.assertEqual(component["version"], expected)


if __name__ == "__main__":
    unittest.main()
