import json
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from openapi_examples import assert_safe_examples, signed_url_paths


class OpenApiExamplesTests(unittest.TestCase):
    def test_signed_examples_rejected_without_echoing_credentials(self):
        url = "https://uploads.example.com/audio.mp3?X-Amz-Credential=REDACTED&X-Amz-Security-Token=REDACTED&X-Amz-Signature=REDACTED"
        example = {"schema": {"example": url}, "examples": [{"value": url}]}
        self.assertEqual(list(signed_url_paths(example)), ["$/schema/example", "$/examples/0/value"])
        with self.assertRaises(ValueError) as error:
            assert_safe_examples(example)
        self.assertNotIn(url, str(error.exception))
        self.assertNotIn("REDACTED", str(error.exception))

    def test_unsigned_urls_and_schema_contract_preserved(self):
        example = {"type": "string", "format": "uri", "example": "https://uploads.example.com/audio.mp3", "nullable": True}
        assert_safe_examples(example)
        self.assertEqual(example["format"], "uri")
        self.assertTrue(example["nullable"])

    def test_all_published_contracts_are_sanitized(self):
        for path in [ROOT / "api/upstream/yoto.openapi.json", ROOT / "api/overlays/contract.json", *sorted((ROOT / "api/effective").glob("*.json"))]:
            with self.subTest(path=path.relative_to(ROOT)):
                assert_safe_examples(json.loads(path.read_text()))


if __name__ == "__main__":
    unittest.main()
