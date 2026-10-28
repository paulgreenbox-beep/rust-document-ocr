"""Source-only tests; no runtime, network, Cargo or user documents."""
import importlib.util
import tempfile
import unittest
from pathlib import Path

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


if __name__ == "__main__":
    unittest.main()
