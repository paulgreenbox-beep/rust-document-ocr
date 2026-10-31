"""Source-only tests; no runtime, network, Cargo or user documents."""
import importlib.util
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import vcpkg_inventory as receipts
from public_vcpkg_inventory import public_projection
from public_vcpkg_inventory import _validate_public_projection
from verify_vcpkg_capture import verify_capture

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

    def metadata(self, triplet=None):
        triplet = triplet or self.triplet
        installed = self.root / "installed"
        listings = {p.name: p.read_text(encoding="utf-8") for p in self.info.glob(f"*_{triplet}.list")}
        data = {}
        for record in self.records:
            if "Feature" in record or record["Architecture"] != triplet:
                continue
            share = installed / triplet / "share" / record["Package"]
            data[record["Package"]] = {"abi_info": (share / "vcpkg_abi_info.txt").read_bytes(),
                                       "spdx": (share / "vcpkg.spdx.json").read_bytes()}
        files = {}
        for text in listings.values():
            for rel in text.splitlines():
                if rel.endswith(("/", "\\")):
                    continue
                raw = (installed / rel).read_bytes()
                files[rel] = {"size": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}
        return [(installed / "vcpkg/status").read_text(encoding="utf-8"), listings, triplet, data, files]


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
        # Tighten the original PR #2 regression: missing core is now diagnostic
        # rejection, not an accepted component with an empty status dictionary.
        with tempfile.TemporaryDirectory() as directory:
            fixture = SyntheticVcpkg(directory)
            fixture.add_port("first", "1", port_version=2)
            fixture.add_port("second", "2")
            components, warnings = fixture.collect()
            self.assertEqual(warnings, [])
            by_id = {c["id"]: c for c in components}
            self.assertEqual(by_id["vcpkg:first"]["version"], "1#2")
            self.assertEqual(by_id["vcpkg:second"]["evidence"]["installed_status"]["Version"], "2")
            fixture.flush([fixture.records[1]])
            with self.assertRaises(ValueError):
                fixture.collect()


class ReceiptValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.fixture = SyntheticVcpkg(self.temp.name)
        self.fixture.add_port("alpha", "1.2.3", port_version=6)
        self.fixture.add_port("beta", "4.5.6")

    def validate(self, args=None):
        return receipts.validate_metadata(*(args or self.fixture.metadata()))

    def test_same_package_two_triplets(self):
        other = "x64-windows"
        self.fixture.add_port("alpha", "9.9", triplet=other)
        selected = {r["id"]: r for r in self.validate()}
        self.assertEqual(selected["vcpkg:alpha"]["version"], "1.2.3#6")
        alternate = receipts.validate_metadata(*self.fixture.metadata(other))
        self.assertEqual(alternate[0]["evidence"]["installed_status"]["Version"], "9.9")
        self.assertEqual(alternate[0]["evidence"]["triplet"], other)

    def test_feature_before_and_after_core_cannot_replace_core(self):
        feature = {"Package": "alpha", "Feature": "tool", "Version": "wrong",
                   "Abi": "wrong", "Architecture": self.fixture.triplet, "Status": "install ok installed"}
        for before in (True, False):
            records = [feature, *self.fixture.records] if before else [*self.fixture.records, feature]
            self.fixture.flush(records)
            alpha = self.validate()[0]
            self.assertEqual(alpha["version"], "1.2.3#6")
            self.assertEqual(alpha["evidence"]["installed_features"], ["core", "tool"])

    def test_conflicting_core_is_rejected(self):
        conflict = dict(self.fixture.records[0], Version="wrong")
        self.fixture.flush([*self.fixture.records, conflict])
        with self.assertRaisesRegex(ValueError, "core status"):
            self.validate()

    def test_missing_core_is_rejected(self):
        self.fixture.flush([self.fixture.records[1]])
        with self.assertRaises(ValueError):
            self.validate()

    def test_raw_version_port_version_and_list_names(self):
        args = self.fixture.metadata()
        rows = self.validate(args)
        alpha, beta = rows
        self.assertEqual(alpha["evidence"]["installed_status"]["Version"], "1.2.3")
        self.assertEqual(alpha["evidence"]["installed_status"]["Port-Version"], "6")
        self.assertNotIn("Port-Version", beta["evidence"]["installed_status"])
        name = next(n for n in args[1] if n.startswith("alpha_"))
        args[1][name.replace("_1.2.3_", "_1.2.3#6_")] = args[1].pop(name)
        self.assertEqual(self.validate(args)[0]["version"], "1.2.3#6")

    def test_release_debug_hash_and_path_are_not_collapsed(self):
        alpha = self.validate()[0]
        release = alpha["evidence"]["archives"][0]
        debug = alpha["evidence"]["debug_archives"][0]
        self.assertEqual(release["path"], f"{self.fixture.triplet}/lib/alpha.lib")
        self.assertEqual(debug["path"], f"{self.fixture.triplet}/debug/lib/alpha.lib")
        self.assertNotEqual(release["sha256"], debug["sha256"])
        expected = hashlib.sha256(f"alpha.lib:{release['sha256']}".encode()).hexdigest().upper()
        self.assertEqual(alpha["sha256"], expected)

    def test_component_dictionaries_are_independent(self):
        args = self.fixture.metadata()
        rows = self.validate(args)
        untouched = json.dumps(rows[1], sort_keys=True)
        rows[0]["evidence"]["installed_status"]["Version"] = "mutated"
        rows[0]["evidence"]["archives"][0]["sha256"] = "mutated"
        self.assertEqual(json.dumps(rows[1], sort_keys=True), untouched)
        self.assertEqual(self.validate(args)[0]["version"], "1.2.3#6")

    def test_abi_receipt_mismatch_is_not_pass(self):
        args = self.fixture.metadata()
        args[3]["alpha"]["abi_info"] += b"changed"
        with self.assertRaisesRegex(ValueError, "ABI receipt"):
            self.validate(args)

    def test_spdx_identity_mismatch_is_not_pass(self):
        args = self.fixture.metadata()
        sbom = json.loads(args[3]["alpha"]["spdx"])
        sbom["packages"][1]["name"] = "beta:" + self.fixture.triplet
        args[3]["alpha"]["spdx"] = json.dumps(sbom).encode()
        with self.assertRaisesRegex(ValueError, "SPDX identity"):
            self.validate(args)

    def test_spdx_file_hash_mismatch_is_not_pass(self):
        args = self.fixture.metadata()
        args[4][f"{self.fixture.triplet}/lib/alpha.lib"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "SPDX recorded SHA256"):
            self.validate(args)

    def test_missing_receipts_are_unavailable(self):
        args = self.fixture.metadata()
        args[3]["alpha"] = {}
        checks = self.validate(args)[0]["evidence"]["receipt_checks"]
        self.assertEqual(checks["abi"]["status"], "UNAVAILABLE")
        self.assertEqual(checks["spdx_identity"]["status"], "UNAVAILABLE")

    def test_missing_build_info_is_not_checked(self):
        checks = self.validate()[0]["evidence"]["receipt_checks"]
        self.assertEqual(checks["build_info"]["status"], "NOT CHECKED")
        self.assertEqual([c["status"] for c in checks["spdx_files"] if c["path"].endswith("/BUILD_INFO")], ["NOT CHECKED"])

    def test_unspecified_license_is_not_replaced(self):
        self.assertEqual(self.validate()[0]["license"], "LicenseRef-vcpkg-null")

    def test_unsafe_and_wrong_triplet_paths_are_rejected(self):
        for relative in ("../escape", "x64/lib/../../escape", "C:/private/value.lib", "//server/share/value.lib", "other/lib/value.lib",
                         f"{self.fixture.triplet}/lib/CON.lib", f"{self.fixture.triplet}/lib/trailing./x.lib"):
            with self.subTest(relative=relative), self.assertRaises(ValueError):
                receipts.safe_relative(relative, self.fixture.triplet)

    def test_output_inside_input_rejected_before_write(self):
        output = self.fixture.root / "new-output"
        argv = ["collector", "--vcpkg-root", str(self.fixture.root), "--output-root", str(output)]
        with patch("sys.argv", argv), self.assertRaises(ValueError):
            inventory.main()
        self.assertFalse(output.exists())

    def test_added_unlisted_release_archive_is_detected(self):
        self.fixture.write(self.fixture.root / "installed" / self.fixture.triplet / "lib/extra.lib", "extra fixture")
        with self.assertRaisesRegex(ValueError, "archive membership"):
            self.fixture.collect()

    def test_orphan_list_is_detected(self):
        args = self.fixture.metadata()
        args[1][f"orphan_1_{self.fixture.triplet}.list"] = ""
        with self.assertRaisesRegex(ValueError, "installed lists"):
            self.validate(args)

    def test_list_version_mismatch_is_rejected(self):
        args = self.fixture.metadata()
        name = next(n for n in args[1] if n.startswith("alpha_"))
        args[1][name.replace("_1.2.3_", "_9.9_")] = args[1].pop(name)
        with self.assertRaisesRegex(ValueError, "version-mismatched"):
            self.validate(args)

    def test_legacy_aggregate_preserves_case_sort_and_final_lf_rule(self):
        archives = [{"name": "b.lib", "sha256": "a" * 64}, {"name": "A.lib", "sha256": "b" * 64}]
        expected = hashlib.sha256(("A.lib:" + "B" * 64 + "\nb.lib:" + "A" * 64).encode()).hexdigest().upper()
        self.assertEqual(receipts.release_aggregate(archives), expected)
        with self.assertRaises(ValueError):
            receipts.release_aggregate([archives[0], dict(archives[0], name="B.lib")])

    def test_historical_inputs_are_not_modified(self):
        def hashes():
            return {p.relative_to(self.fixture.root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in self.fixture.root.rglob("*") if p.is_file()}
        before = hashes()
        self.fixture.collect()
        self.assertEqual(before, hashes())

    def test_notice_destination_symlink_escape_rejected(self):
        source = self.fixture.base / "LICENSE"
        source.write_text("synthetic notice", encoding="utf-8")
        notices = self.fixture.output / "notices"
        destination = notices / "test-LICENSE"
        outside = self.fixture.base / "outside-notice"
        original_resolve = Path.resolve

        def redirected(path, *args, **kwargs):
            # Model a dangling destination symlink without Windows privileges.
            return original_resolve(outside if path == destination else path, *args, **kwargs)

        with patch.object(Path, "resolve", redirected), self.assertRaises(ValueError):
            inventory.copy_notice(source, notices, "test")
        self.assertFalse(outside.exists())

    def test_manifest_creation_remains_exclusive_after_exists_race(self):
        output = self.fixture.output
        output.mkdir()
        manifest = output / "runtime-manifest.local.json"
        manifest.write_text("preserve existing evidence", encoding="utf-8")
        original_exists = Path.exists

        def missed_check(path):
            return False if path == manifest else original_exists(path)

        argv = ["collector", "--vcpkg-root", str(self.fixture.root), "--output-root", str(output),
                "--cargo-registry", str(self.fixture.base / "empty-registry")]
        with patch("sys.argv", argv), patch.object(Path, "exists", missed_check), \
                patch.object(inventory, "crate_inventory", return_value=[]), \
                patch.object(inventory, "vcpkg_inventory", return_value=([], [])), self.assertRaises(FileExistsError):
            inventory.main()
        self.assertEqual(manifest.read_text(encoding="utf-8"), "preserve existing evidence")


def synthetic_projection_source():
    """One-package derivative-input fixture, not a real local inventory."""
    triplet, name = "x64-windows-static-md", "alpha"
    status = {"Version": "1.2.3", "Port-Version": "2", "Abi": "a" * 64, "Architecture": triplet}
    release = {"name": "alpha.lib", "configuration": "release", "path": f"{triplet}/lib/alpha.lib", "size": 12, "sha256": "b" * 64}
    debug = {"name": "alpha.lib", "configuration": "debug", "path": f"{triplet}/debug/lib/alpha.lib", "size": 15, "sha256": "c" * 64}
    return {
        "local_only": True, "distribution_status": "blocked",
        "started_utc": "2026-10-31T01:02:03+00:00", "completed_utc": "2026-10-31T01:02:04+00:00",
        "scope": {"triplet": triplet, "all_installed_files_for_selected_triplet_hashed": True},
        "packages": [{"package": name, "triplet": triplet, "version": "1.2.3#2", "abi": status["Abi"],
                      "files": [{key: row[key] for key in ("path", "size", "sha256")} for row in (release, debug)],
                      "status_record": {"Package": name, "Status": "install ok installed", **status}}],
        "static_components": [{"id": "vcpkg:alpha", "version": "1.2.3#2", "license": "LicenseRef-vcpkg-null",
                               "distribution_status": "blocked", "evidence": {"triplet": triplet, "installed_status": dict(status),
                               "archives": [release], "debug_archives": [debug]}}],
        "historical_comparison": [{"id": "vcpkg:alpha", "historical_version": "1.2.3#2", "current_version": "1.2.3#2",
                                   "top_level_version_match": True, "historical_installed_status": dict(status),
                                   "current_installed_status": dict(status), "different_installed_status_fields": [],
                                   "historical_archive_comparisons": [{"path": release["path"], "historical_sha256": release["sha256"],
                                                                      "current_sha256": release["sha256"], "match": True}],
                                   "all_historical_archives_match": True, "current_release_archives_added": [],
                                   "historical_release_archives_missing": [], "exact_release_archive_set_and_hash_match": True}],
        "summary": {"installed_ports": 1, "release_archives": 1, "debug_archives": 1, "historical_archive_hashes_checked": 1,
                    "historical_status_discrepancies": 0, "all_historical_archive_hashes_match": True,
                    "all_historical_release_archive_sets_and_hashes_match": True, "installed_files_hashed": 2},
        "metadata_consistency": {"all_installed_payload_hashes_rechecked": True, "installed_payload_files_rechecked": 2},
    }


class PublicProjectionTests(unittest.TestCase):
    def test_committed_public_receipt_has_own_digest(self):
        path = Path(__file__).resolve().parents[1] / "reports/vcpkg-inventory-refresh-20261031/VCPKG_INVENTORY_PUBLIC.json"
        raw = path.read_bytes()
        own_sha, filename = path.with_suffix(".json.sha256").read_text(encoding="utf-8").strip().split("  ", 1)
        self.assertEqual(own_sha, hashlib.sha256(raw).hexdigest())
        self.assertEqual(filename, path.name)
        document = json.loads(raw)
        _validate_public_projection(document)
        self.assertNotEqual(own_sha, document["source_local_manifest_sha256"])

    def test_public_projection_allowlist_omits_private_fields(self):
        source = synthetic_projection_source()
        sentinel = "DO_NOT_PUBLISH_PRIVATE_SENTINEL"
        source.update({"machine_name": sentinel, "stderr": sentinel, "source_path": sentinel, "private_error": sentinel})
        source["scope"]["vcpkg_root"] = sentinel
        source["packages"][0]["status_record"]["Description"] = sentinel
        source["static_components"][0]["evidence"]["archives"][0]["private_path"] = sentinel
        result = public_projection(source, "d" * 64)
        self.assertNotIn(sentinel, json.dumps(result))
        self.assertTrue(result["derived_from_local_observation"])
        self.assertFalse(result["historical_link_receipt"])
        self.assertEqual(result["distribution_status"], "blocked")
        self.assertEqual(result["source_local_manifest_sha256"], "d" * 64)

    def test_license_ref_is_preserved_verbatim(self):
        row = public_projection(synthetic_projection_source(), "d" * 64)["packages"][0]
        self.assertEqual(row["license"], "LicenseRef-vcpkg-null")

    def test_unsafe_path_does_not_leak_through_errors(self):
        for path in ("../private.lib", "C:/private/alpha.lib", "//server/share/alpha.lib"):
            source = synthetic_projection_source()
            source["static_components"][0]["evidence"]["archives"][0]["path"] = path
            with self.assertRaises(ValueError) as caught:
                public_projection(source, "d" * 64)
            self.assertNotIn(path, str(caught.exception))

    def test_summary_boolean_is_not_trusted_blindly(self):
        source = synthetic_projection_source()
        source["historical_comparison"][0]["historical_archive_comparisons"][0]["historical_sha256"] = "e" * 64
        with self.assertRaises(ValueError):
            public_projection(source, "d" * 64)

    def test_projection_does_not_mutate_local_or_historical_input(self):
        source = synthetic_projection_source()
        before = json.dumps(source, sort_keys=True)
        result = public_projection(source, "d" * 64)
        result["packages"][0]["version"] = "mutated"
        self.assertEqual(before, json.dumps(source, sort_keys=True))

    def test_source_must_remain_local_and_distribution_blocked(self):
        source = synthetic_projection_source()
        source["distribution_status"] = "verified"
        with self.assertRaises(ValueError):
            public_projection(source, "d" * 64)

    def test_archive_size_and_hash_must_match_package_files(self):
        for key, value in (("size", 99), ("sha256", "f" * 64)):
            source = synthetic_projection_source()
            source["static_components"][0]["evidence"]["debug_archives"][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                public_projection(source, "d" * 64)

    def test_reserved_windows_names_and_trailing_dot_rejected(self):
        for basename in ("CON.lib", "COM1.lib", "trailing./alpha.lib"):
            source = synthetic_projection_source()
            source["packages"][0]["files"][0]["path"] = f"x64-windows-static-md/lib/{basename}"
            with self.subTest(basename=basename), self.assertRaises(ValueError):
                public_projection(source, "d" * 64)

    def test_added_package_library_requires_public_archive_record(self):
        source = synthetic_projection_source()
        source["packages"][0]["files"].append({"path": "x64-windows-static-md/lib/extra.lib", "size": 7, "sha256": "e" * 64})
        with self.assertRaises(ValueError):
            public_projection(source, "d" * 64)

    def test_casefold_duplicate_payload_paths_rejected(self):
        source = synthetic_projection_source()
        duplicate = dict(source["packages"][0]["files"][0])
        duplicate["path"] = duplicate["path"].replace("alpha.lib", "ALPHA.lib")
        source["packages"][0]["files"].append(duplicate)
        with self.assertRaises(ValueError):
            public_projection(source, "d" * 64)


class SavedCaptureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        fixture = SyntheticVcpkg(self.temp.name)
        fixture.add_port("alpha", "1.2")
        status, lists, triplet, port_receipts, files = fixture.metadata()
        components = receipts.validate_metadata(status, lists, triplet, port_receipts, files)
        self.capture = fixture.base / "capture"
        manifest = {"scope": {"triplet": triplet}, "packages": [{"package": "alpha", "triplet": triplet,
                    "files": [{"path": path, **value} for path, value in files.items()]}]}
        parts = {"vcpkg-inventory.local.json": json.dumps(manifest).encode(),
                 "evidence/vcpkg/installed-status.txt": status.encode(),
                 "evidence/historical/WINDOWS_PORTABLE_CANDIDATE_MANIFEST.json": json.dumps({"static_components": components}).encode()}
        parts.update({f"evidence/vcpkg/info/{name}": value.encode() for name, value in lists.items()})
        for port, values in port_receipts.items():
            parts[f"evidence/vcpkg/share/{port}/vcpkg_abi_info.txt"] = values["abi_info"]
            parts[f"evidence/vcpkg/share/{port}/vcpkg.spdx.json"] = values["spdx"]
        for relative, raw in parts.items():
            fixture.write(self.capture / relative, raw)
        index = "\n".join(f"{hashlib.sha256(raw).hexdigest()}  {relative}" for relative, raw in sorted(parts.items())) + "\n"
        fixture.write(self.capture / "SHA256SUMS.txt", index)
        self.manifest_sha = hashlib.sha256(parts["vcpkg-inventory.local.json"]).hexdigest()
        self.index_sha = hashlib.sha256(index.encode()).hexdigest()

    def test_saved_receipts_replayed_without_opening_payload(self):
        original = Path.read_bytes

        def metadata_only(path):
            self.assertNotEqual(path.suffix, ".lib", "Replay must never reopen payload")
            return original(path)

        with patch.object(Path, "read_bytes", metadata_only):
            result = verify_capture(self.capture, self.manifest_sha, self.index_sha)
        self.assertEqual(result["packages"], 1)
        self.assertEqual(result["release_archives"], 1)
        self.assertEqual(result["debug_archives"], 1)
        self.assertEqual(result["real_payload_files_rehashed"], 0)
        self.assertEqual(result["abi_receipts_passed"], 1)
        self.assertEqual(result["build_info_not_checked"], 1)

    def test_saved_receipt_byte_tamper_is_rejected(self):
        (self.capture / "evidence/vcpkg/installed-status.txt").write_text("changed", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            verify_capture(self.capture, self.manifest_sha, self.index_sha)


if __name__ == "__main__":
    unittest.main()
