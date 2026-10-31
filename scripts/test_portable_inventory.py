"""Source-only tests; no runtime, network, Cargo or user documents."""
import importlib.util
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from unittest import mock

spec = importlib.util.spec_from_file_location("inventory", Path(__file__).with_name("collect-portable-inventory.py"))
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class SyntheticVcpkg:
    """Tiny text-only installed tree; never reads real packages or binaries."""

    triplet = "x64-windows-static-md"

    def __init__(self, directory):
        self.base = Path(directory)
        self.root = self.base / "vcpkg"
        self.repo = self.base / "repo"
        self.output = self.base / "inventory"
        self.info = self.root / "installed/vcpkg/info"
        self.info.mkdir(parents=True)
        self.records = []
        self.lists = {}

    def write(self, path, data):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data if isinstance(data, bytes) else data.encode("utf-8"))

    def add_port(self, name, version, port_version=None, triplet=None):
        triplet = triplet or self.triplet
        receipt = f"features core\npackage {name}\ntriplet {triplet}\nversion {version}\n".encode()
        abi = hashlib.sha256(receipt).hexdigest()
        fields = {"Package": name, "Version": version, "Architecture": triplet,
                  "Abi": abi, "Status": "install ok installed"}
        if port_version is not None:
            fields["Port-Version"] = str(port_version)
        self.records.append(fields)
        installed = self.root / "installed"
        paths = []
        files = []
        for config in ("lib", "debug/lib"):
            rel = f"{triplet}/{config}/{name}.lib"
            # The suffix exercises receipt matching; these are NOT native libs.
            payload = f"synthetic text fixture: {name} {version} {config}\n".encode()
            self.write(installed / rel, payload)
            paths.append(rel)
            files.append({"fileName": f"./{config}/{name}.lib",
                          "SPDXID": f"SPDXRef-binary-file-{len(files)}",
                          "checksums": [{"algorithm": "SHA256", "checksumValue": hashlib.sha256(payload).hexdigest()}]})
        share = installed / triplet / "share" / name
        self.write(share / "vcpkg_abi_info.txt", receipt)
        self.write(share / "copyright", "Synthetic notice; no licensed payload.\n")
        files.append({"fileName": "./BUILD_INFO", "SPDXID": "SPDXRef-binary-file-build",
                      "checksums": [{"algorithm": "SHA256", "checksumValue": "b" * 64}]})
        sbom = {"packages": [{"SPDXID": "SPDXRef-port", "name": name, "versionInfo": version,
                              "licenseConcluded": "LicenseRef-vcpkg-null"},
                             {"SPDXID": "SPDXRef-binary", "name": f"{name}:{triplet}", "versionInfo": abi}],
                "files": files}
        self.write(share / "vcpkg.spdx.json", json.dumps(sbom))
        listing = self.info / f"{name}_{version}_{triplet}.list"
        self.write(listing, "\n".join(paths) + "\n")
        self.lists[(name, triplet)] = listing
        self.write(self.root / "ports" / name / "vcpkg.json",
                   json.dumps({"name": name, "version": version, "license": "LicenseRef-vcpkg-null"}))
        self.flush()
        return fields

    def flush(self, records=None):
        records = self.records if records is None else records
        self.write(self.root / "installed/vcpkg/status",
                   "\n\n".join("\n".join(f"{k}: {v}" for k, v in r.items()) for r in records) + "\n")
        names = sorted({r["Package"] for r in self.records if "Feature" not in r and r["Architecture"] == self.triplet})
        self.write(self.repo / "target/release/build/synthetic/output",
                   "\n".join(f"cargo:rustc-link-lib={name}" for name in names) + "\n")

    def collect(self, list_order=None):
        original_glob = Path.glob

        def ordered_glob(directory, pattern):
            items = original_glob(directory, pattern)
            if list_order is not None and directory == self.info:
                priority = {name: i for i, name in enumerate(list_order)}
                return iter(sorted(items, key=lambda p: priority.get(p.name.split("_", 1)[0], len(priority))))
            return items

        with patch.object(inventory, "REPO", self.repo), patch.object(Path, "glob", ordered_glob):
            return inventory.vcpkg_inventory(self.root, self.output, self.output / "notices")


class StatusAssociationRegressionTests(unittest.TestCase):
    def test_each_package_receives_own_status_when_zstd_is_last(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = SyntheticVcpkg(directory)
            expected = {name: fixture.add_port(name, version) for name, version in
                        [("alpha", "1.2.3"), ("beta", "4.5.6"), ("zstd", "7.8.9")]}
            rows, errors = fixture.collect(["alpha", "beta", "zstd"])
            self.assertEqual(errors, [])
            for row in rows:
                name = row["id"].split(":", 1)[1]
                actual = row["evidence"]["installed_status"]
                self.assertEqual(actual["Version"], expected[name]["Version"], name)
                self.assertEqual(actual["Abi"], expected[name]["Abi"], name)

    def test_status_and_list_permutation_do_not_change_association(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = SyntheticVcpkg(directory)
            for name, version in [("alpha", "1"), ("beta", "2"), ("zstd", "3")]:
                fixture.add_port(name, version)
            first, errors = fixture.collect(["alpha", "beta", "zstd"])
            self.assertEqual(errors, [])
            fixture.flush(list(reversed(fixture.records)))
            second, errors = fixture.collect(["zstd", "alpha", "beta"])
            self.assertEqual(errors, [])
            self.assertEqual({r["id"]: r["evidence"]["installed_status"] for r in first},
                             {r["id"]: r["evidence"]["installed_status"] for r in second})


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
