"""The selected project configuration and the shared branch grammar.

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = REPO_ROOT / "scripts"
HOOK = REPO_ROOT / ".githooks" / "pre-push"
BRANCH_SOURCE = REPO_ROOT / "crates" / "kontor-core" / "src" / "branch.rs"

sys.path.insert(0, str(SCRIPTS))
import kontor_project  # noqa: E402

ZERO = "0" * 40
SHA = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4"


def write_project(folder: Path, *, key: str, base: str, check: str | None = None) -> Path:
    path = folder / "orchestration" / "orchestration.yml"
    path.parent.mkdir(parents=True)
    check = check or f"{key.lower()}/publication-branch-title"
    path.write_text(
        "schema_version: 1\n"
        "project:\n"
        f'  key: "{key}"\n'
        "  forge:\n"
        f'    base_branch: "{base}"\n'
        "  check_runs:\n"
        f'    - "{check}"\n',
        encoding="utf-8",
    )
    return path


def run_hook(branch: str, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    env.setdefault("KONTOR_PROJECT_ROOT", str(REPO_ROOT))
    line = f"refs/heads/{branch} {SHA} refs/heads/{branch} {SHA}\n"
    return subprocess.run(
        ["sh", str(HOOK)],
        cwd=REPO_ROOT,
        env=env,
        input=line,
        capture_output=True,
        text=True,
        check=False,
    )


class SelectionTests(unittest.TestCase):
    def test_an_absent_selection_is_refused_by_name(self) -> None:
        with self.assertRaises(kontor_project.ProjectConfigError) as raised:
            kontor_project.selected_config_path(env={}, cwd=REPO_ROOT)
        self.assertIn("KONTOR_CONFIG_DIR", str(raised.exception))
        self.assertIn("KONTOR_CONFIG_FILE", str(raised.exception))

    def test_an_explicit_folder_wins_over_the_environment(self) -> None:
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            write_project(Path(first), key="AAA", base="main")
            write_project(Path(second), key="BBB", base="trunk")
            selected = kontor_project.selected_config_path(
                config_dir=first,
                env={"KONTOR_CONFIG_DIR": second},
                cwd=REPO_ROOT,
            )
            self.assertEqual(selected.parent.parent, Path(first).resolve())

    def test_an_explicit_projection_file_wins_over_a_folder(self) -> None:
        with tempfile.TemporaryDirectory() as folder, tempfile.TemporaryDirectory() as files:
            write_project(Path(folder), key="AAA", base="main")
            projection = Path(files) / "projection.yml"
            projection.write_text(
                "schema_version: 1\n"
                "generated:\n"
                "  source: repository-variables\n"
                "project:\n"
                "  key: EXM\n"
                "  forge:\n"
                "    base_branch: main\n"
                "  check_runs: [exm/publication-branch-title]\n",
                encoding="utf-8",
            )
            selected = kontor_project.selected_config_path(
                config_dir=folder,
                config_file=str(projection),
                env={},
                cwd=REPO_ROOT,
            )
            self.assertEqual(selected, projection.resolve())


class ProjectConfigurationTests(unittest.TestCase):
    def test_a_second_project_key_reads_the_base_and_check_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = write_project(Path(tmp), key="EXM", base="main")
            project = kontor_project.load_project_config(path)
        self.assertEqual(project.key, "EXM")
        self.assertEqual(project.base_branch, "main")
        self.assertEqual(project.check_name, "exm/publication-branch-title")
        self.assertEqual(
            project.provenance["source"],
            "selected-config-folder/orchestration/orchestration.yml",
        )
        self.assertRegex(
            project.provenance["source_revision"], r"^sha256:[0-9a-f]{64}$"
        )
        self.assertEqual(
            project.provenance["source_sha256"],
            project.provenance["source_revision"].removeprefix("sha256:"),
        )

    def test_a_generated_projection_records_its_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "projection.yml"
            path.write_text(
                "schema_version: 1\n"
                "generated:\n"
                "  source: repository-variables\n"
                "  source_revision: abc\n"
                "  variables: [KONTOR_PROJECT_KEY]\n"
                "project:\n"
                "  key: EXM\n"
                "  forge:\n"
                "    base_branch: main\n"
                "  check_runs: [publication-branch-title]\n",
                encoding="utf-8",
            )
            project = kontor_project.load_project_config(path)
        self.assertEqual(project.provenance["source"], "repository-variables")
        self.assertEqual(project.check_name, "publication-branch-title")

    def test_the_yaml_projection_round_trips_generated_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "orchestration" / "orchestration.yml"
            path.parent.mkdir(parents=True)
            path.write_text(
                "schema_version: 1\n"
                "project:\n"
                '  key: "ON"\n'
                "  forge:\n"
                '    base_branch: "2026"\n'
                "  check_runs: [\"on/publication-branch-title\"]\n",
                encoding="utf-8",
            )
            project = kontor_project.load_project_config(path)
            rendered = kontor_project.render_project_projection(
                project, format="yaml"
            )
            self.assertIn('key: "ON"', rendered)
            self.assertIn('base_branch: "2026"', rendered)
            self.assertIn("generated:", rendered)
            self.assertIn(
                'source: "selected-config-folder/orchestration/orchestration.yml"',
                rendered,
            )
            self.assertNotIn(tmp, rendered)

            reloaded_path = Path(tmp) / "projection.yml"
            reloaded_path.write_text(rendered, encoding="utf-8")
            reloaded = kontor_project.load_project_config(reloaded_path)
        self.assertEqual(reloaded.key, "ON")
        self.assertEqual(reloaded.base_branch, "2026")
        self.assertEqual(reloaded.check_name, "on/publication-branch-title")
        self.assertEqual(reloaded.provenance, project.provenance)

    def test_the_json_projection_also_carries_public_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = write_project(Path(tmp), key="EXM", base="main")
            project = kontor_project.load_project_config(path)
            rendered = kontor_project.render_project_projection(
                project, format="json"
            )
            data = json.loads(rendered)
        self.assertEqual(
            data["generated"]["source"],
            "selected-config-folder/orchestration/orchestration.yml",
        )
        self.assertNotIn(tmp, rendered)

    def test_a_config_without_a_project_block_is_refused_by_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "orchestration" / "orchestration.yml"
            path.parent.mkdir(parents=True)
            path.write_text(
                "schema_version: 1\n"
                "fleet: orchestration/fleet.yml\n",
                encoding="utf-8",
            )
            with self.assertRaises(kontor_project.ProjectConfigError) as raised:
                kontor_project.load_project_config(path)
        self.assertIn("no project: block", str(raised.exception))

    def test_a_missing_publication_check_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = write_project(Path(tmp), key="EXM", base="main", check="ci/test")
            with self.assertRaises(kontor_project.ProjectConfigError):
                kontor_project.load_project_config(path)

    def test_key_and_tracker_key_must_agree(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "orchestration" / "orchestration.yml"
            path.parent.mkdir(parents=True)
            path.write_text(
                "schema_version: 1\n"
                "project:\n"
                "  key: EXM\n"
                "  tracker:\n"
                "    project_key: OTH\n"
                "  forge:\n"
                "    base_branch: main\n"
                "  check_runs: [exm/publication-branch-title]\n",
                encoding="utf-8",
            )
            with self.assertRaises(kontor_project.ProjectConfigError):
                kontor_project.load_project_config(path)


class CommittedProjectionTests(unittest.TestCase):
    def test_the_committed_projection_is_reproducible_from_its_source(self) -> None:
        committed_path = REPO_ROOT / ".github" / "kontor-project.yml"
        committed = kontor_project.load_yaml(committed_path)
        self.assertIsInstance(committed, dict)
        generated = committed["generated"]
        source = REPO_ROOT / generated["source"]
        self.assertTrue(source.is_file(), str(source))
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        self.assertEqual(generated["source_sha256"], digest)
        self.assertEqual(generated["source_revision"], f"sha256:{digest}")
        project = kontor_project.load_project_config(source)
        rendered = kontor_project.render_project_projection(
            project, format="yaml"
        )
        with tempfile.TemporaryDirectory() as tmp:
            regenerated_path = Path(tmp) / "projection.yml"
            regenerated_path.write_text(rendered, encoding="utf-8")
            regenerated = kontor_project.load_yaml(regenerated_path)
        self.assertEqual(regenerated, committed)


class GrammarTests(unittest.TestCase):
    def test_the_types_come_from_kontor_core(self) -> None:
        self.assertEqual(
            kontor_project.load_branch_types(BRANCH_SOURCE),
            ("feat", "fix", "docs", "chore", "refactor", "test", "perf", "build", "ci", "releases"),
        )

    def test_a_changed_kontor_core_source_changes_the_grammar(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "branch.rs"
            source.write_text(
                "closed_enum! {\n"
                '    BranchType, "BranchType" {\n'
                '        Feat => "feat",\n'
                '        Fix => "fix",\n'
                '        Releases => "releases",\n'
                "    }\n"
                "}\n",
                encoding="utf-8",
            )
            self.assertEqual(
                kontor_project.load_branch_types(source),
                ("feat", "fix", "releases"),
            )

    def test_a_missing_grammar_source_is_refused(self) -> None:
        with self.assertRaises(kontor_project.ProjectConfigError):
            kontor_project.load_branch_types("/nonexistent/branch.rs")

    def test_the_grammar_is_project_neutral(self) -> None:
        types = kontor_project.load_branch_types(BRANCH_SOURCE)
        for branch in (
            "feat/EXM-1-do-work",
            "fix/EXM-42-repair-empty-rows",
            "releases/v1.2.3",
            "releases/EXM-7-hotfix",
        ):
            with self.subTest(branch=branch):
                self.assertIsNone(
                    kontor_project.branch_refusal(
                        branch, project_key="EXM", branch_types=types
                    )
                )
        self.assertEqual(
            kontor_project.branch_refusal(
                "feat/ASMA-1-do-work", project_key="EXM", branch_types=types
            ),
            "branch_project_mismatch",
        )

    def test_the_refusal_codes_match_the_checker(self) -> None:
        types = kontor_project.load_branch_types(BRANCH_SOURCE)
        cases = {
            "cat-11": "branch_shape_invalid",
            "feature/EXM-1-x": "branch_type_unknown",
            "feat/add-widget": "branch_key_missing",
            "feat/exm-1-add-widget": "branch_key_not_canonical",
            "feat/EXM-0-add-widget": "branch_key_not_canonical",
            "feat/EXM-1-Add-Widget": "branch_slug_invalid",
            "releases/latest": "release_branch_invalid",
        }
        for branch, expected in cases.items():
            with self.subTest(branch=branch):
                self.assertEqual(
                    kontor_project.branch_refusal(
                        branch, project_key="EXM", branch_types=types
                    ),
                    expected,
                )


class HookTests(unittest.TestCase):
    def _env(self, config: Path) -> dict[str, str]:
        env = os.environ.copy()
        env.update(
            {
                "PASEO_AGENT_ID": "test-agent",
                "KONTOR_CONFIG_DIR": str(config),
                "KONTOR_BRANCH_GRAMMAR_SOURCE": str(BRANCH_SOURCE),
            }
        )
        env.pop("KONTOR_ALLOW_AGENT_PUSH", None)
        env.pop("KONTOR_CONFIG_FILE", None)
        return env

    def test_the_hook_passes_for_another_project_key(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write_project(Path(tmp), key="EXM", base="main")
            result = run_hook("fix/EXM-7-repair-rows", self._env(Path(tmp)))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("BLOCKED", result.stderr)

    def test_the_hook_refuses_a_foreign_key(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write_project(Path(tmp), key="EXM", base="main")
            result = run_hook("fix/ASMA-7-repair-rows", self._env(Path(tmp)))
        self.assertEqual(result.returncode, 1)
        self.assertIn("branch_project_mismatch", result.stderr)

    def test_the_hook_refuses_the_configured_base_branch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write_project(Path(tmp), key="EXM", base="main")
            result = run_hook("main", self._env(Path(tmp)))
        self.assertEqual(result.returncode, 1)
        self.assertIn("BLOCKED: agent push to refs/heads/main", result.stderr)

    def test_the_hook_defaults_to_the_committed_projection(self) -> None:
        env = os.environ.copy()
        env.update({"PASEO_AGENT_ID": "test-agent"})
        for key in (
            "KONTOR_CONFIG_DIR",
            "KONTOR_CONFIG_FILE",
            "KONTOR_ALLOW_AGENT_PUSH",
        ):
            env.pop(key, None)
        result = run_hook("fix/ASMA-7-repair-rows", env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("BLOCKED", result.stderr)

    def test_the_escape_hatch_is_deliberate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write_project(Path(tmp), key="EXM", base="main")
            env = self._env(Path(tmp))
            env["KONTOR_ALLOW_AGENT_PUSH"] = "1"
            result = run_hook("anything", env)
        self.assertEqual(result.returncode, 0, result.stderr)


class TestLanesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        spec = importlib.util.spec_from_file_location(
            "test_lanes", SCRIPTS / "test-lanes.py"
        )
        assert spec is not None and spec.loader is not None
        cls.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.module)

    def test_the_full_lane_does_not_require_a_project_selection(self) -> None:
        completed = subprocess.CompletedProcess([], 0)
        with mock.patch.dict(
            os.environ, {"KONTOR_CONFIG_DIR": "", "KONTOR_CONFIG_FILE": ""}
        ), mock.patch.object(
            sys, "argv", ["test-lanes.py", "--lane", "full"]
        ), mock.patch.object(
            self.module.subprocess, "run", return_value=completed
        ):
            self.assertEqual(self.module.main(), 0)

    def test_the_default_base_comes_from_the_project(self) -> None:
        self.assertEqual(
            self.module.base_candidates("main"), ("origin/main", "main")
        )
        self.assertEqual(
            self.module.base_candidates("trunk"), ("origin/trunk", "trunk")
        )


if __name__ == "__main__":
    unittest.main()
