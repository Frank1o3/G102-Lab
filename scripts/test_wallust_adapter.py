import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "scripts" / "g102-control-wallust"


class WallustAdapterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="g102-wallust-test-")
        self.root = Path(self.temp.name)
        self.palette = self.root / "palette.json"
        self.called = self.root / "set-args.json"
        self.fake_cli = self.root / "fake-g102-control"
        self.fake_cli.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "args = sys.argv[1:]\n"
            "if args[-2:] == ['rgb', 'effects']:\n"
            "    print('Cluster 0:')\n"
            "    print('    effect index 1: Static color (ID 0x0001), capabilities 0x0000, period 0')\n"
            "    print('    effect index 2: Color cycle (ID 0x0003), capabilities 0x0000, period 0')\n"
            "elif 'rgb' in args and 'set' in args:\n"
            "    with open(os.environ['G102_TEST_SET_ARGS'], 'w') as f: json.dump(args, f)\n"
            "    print('mock setter invoked')\n"
            "else:\n"
            "    raise SystemExit(64)\n",
            encoding="utf-8",
        )
        self.fake_cli.chmod(0o755)
        self.env = os.environ.copy()
        self.env["G102_TEST_SET_ARGS"] = str(self.called)

    def tearDown(self):
        self.temp.cleanup()

    def write_palette(self, color="#89b4fa"):
        self.palette.write_text(
            json.dumps(
                {
                    "schema": "org.g102-control.palette/v1",
                    "source": "wallust",
                    "colors": {"accent": color, "background": "#1e1e2e"},
                }
            ),
            encoding="utf-8",
        )

    def run_adapter(self, *args):
        return subprocess.run(
            [sys.executable, str(ADAPTER), *map(str, args)],
            text=True,
            capture_output=True,
            env=self.env,
            check=False,
        )

    def test_valid_palette_calls_only_reported_static_slot(self):
        self.write_palette()
        result = self.run_adapter(
            "--no-config",
            "--palette", self.palette,
            "--color-key", "accent",
            "--cluster", "0",
            "--effect-index", "1",
            "--g102-control", self.fake_cli,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads(self.called.read_text(encoding="utf-8"))
        self.assertEqual(args[-5:], ["rgb", "set", "0", "1", "89B4FA"])
        self.assertIn("Applying Wallust accent", result.stderr)

    def test_malformed_palette_does_not_invoke_cli(self):
        self.palette.write_text("{not json", encoding="utf-8")
        result = self.run_adapter(
            "--no-config", "--palette", self.palette,
            "--color-key", "accent", "--cluster", "0", "--effect-index", "1",
            "--g102-control", self.fake_cli,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("valid palette JSON", result.stderr)
        self.assertFalse(self.called.exists())

    def test_invalid_color_and_unreported_non_static_slot_are_refused(self):
        self.write_palette("#12345678")
        args = [
            "--no-config", "--palette", self.palette, "--color-key", "accent",
            "--cluster", "0", "--effect-index", "1", "--g102-control", self.fake_cli,
        ]
        invalid_color = self.run_adapter(*args)
        self.assertEqual(invalid_color.returncode, 2)
        self.assertFalse(self.called.exists())

        self.write_palette()
        args[args.index("1", args.index("--effect-index") + 1)] = "2"
        invalid_target = self.run_adapter(*args)
        self.assertEqual(invalid_target.returncode, 2)
        self.assertIn("not reported as Static color", invalid_target.stderr)
        self.assertFalse(self.called.exists())

    def test_enabled_config_supplies_palette_and_target(self):
        self.write_palette()
        config = self.root / "config.toml"
        config.write_text(
            "version = 1\n[wallust]\nenabled = true\n"
            f"palette_file = '{self.palette}'\ncolor_key = 'accent'\ncluster = 0\neffect_index = 1\n",
            encoding="utf-8",
        )
        result = self.run_adapter("--config", config, "--g102-control", self.fake_cli)
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads(self.called.read_text(encoding="utf-8"))
        self.assertIn("--config", args)
        self.assertIn(str(config), args)

    def test_disabled_config_requires_explicit_target(self):
        self.write_palette()
        config = self.root / "config.toml"
        config.write_text("version = 1\n[wallust]\nenabled = false\n", encoding="utf-8")
        result = self.run_adapter("--config", config, "--g102-control", self.fake_cli)
        self.assertEqual(result.returncode, 2)
        self.assertIn("disabled or incomplete", result.stderr)
        self.assertFalse(self.called.exists())


if __name__ == "__main__":
    unittest.main()
