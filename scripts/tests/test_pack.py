"""Tests for the workflow pack render and check tool (ASMA-8567).

Run from the repository root:

    python3 -m unittest discover -s scripts/tests -v

The render tests use a disposable empty project. The check tests add the public
pack words plus a router, so a failure names only the injected defect. The ASMA
case is the repository's public schema fixture; it carries no private path.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from datetime import date, timedelta
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[2]
PACK = REPO_ROOT / "scripts" / "pack.py"
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import pack  # noqa: E402
EXAMPLES = REPO_ROOT / "config" / "examples"
ASMA_FIXTURE = REPO_ROOT / "tests" / "fixtures" / "config" / "asma-orchestration.yml"
PUBLIC_DOCS = ("WORKFLOW.md", "GLOSSARY.md", "PRINCIPLES.md")


def clean_env(**overrides: str) -> dict[str, str]:
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        "LANG": os.environ.get("LANG", "C.UTF-8"),
    }
    env.update(overrides)
    return env


class YamlReaderTests(unittest.TestCase):
    def test_an_unindented_sequence_under_a_key_is_read(self) -> None:
        data = pack.load_yaml_text(
            "seats:\n"
            "- role_slot_id: lsa\n"
            "  role_code: LSA\n"
            "- role_slot_id: tpm\n"
            "  role_code: TPM\n"
        )
        self.assertEqual(
            [seat["role_slot_id"] for seat in data["seats"]], ["lsa", "tpm"]
        )
        self.assertEqual(data["seats"][0]["role_code"], "LSA")

    def test_an_indented_flow_sequence_under_a_key_is_read(self) -> None:
        data = pack.load_yaml_text("tags:\n  [one, two]\n")
        self.assertEqual(data["tags"], ["one", "two"])

    def test_an_unclosed_flow_sequence_is_refused(self) -> None:
        with self.assertRaises(pack.YamlError):
            pack.load_yaml_text("owner: [Platform Owner\n")

    def test_an_unclosed_quoted_scalar_is_refused(self) -> None:
        with self.assertRaises(pack.YamlError):
            pack.load_yaml_text("owner: 'Platform Owner\n")

    def test_an_unquoted_mapping_separator_is_refused_in_the_fallback(
        self,
    ) -> None:
        with mock.patch.object(pack, "_pyyaml", None):
            with self.assertRaises(pack.YamlError):
                pack.load_yaml_text("owner: Platform: Owner\n")

    def test_content_after_a_closing_quote_is_refused_in_the_fallback(
        self,
    ) -> None:
        with mock.patch.object(pack, "_pyyaml", None):
            with self.assertRaises(pack.YamlError):
                pack.load_yaml_text("owner: 'Platform' 'Owner'\n")

    def test_a_doubled_single_quote_is_read_in_the_fallback(self) -> None:
        with mock.patch.object(pack, "_pyyaml", None):
            data = pack.load_yaml_text("owner: 'It''s fine'\n")
        self.assertEqual(data["owner"], "It's fine")


    def test_pyyaml_is_preferred_when_available(self) -> None:
        fake = mock.Mock()
        fake.safe_load.return_value = {"from": "pyyaml"}
        fake.YAMLError = ValueError
        with mock.patch.object(pack, "_pyyaml", fake):
            data = pack.load_yaml_text("owner: fallback\n")
        self.assertEqual(data, {"from": "pyyaml"})
        fake.safe_load.assert_called_once_with("owner: fallback\n")

    def test_duplicate_mapping_keys_are_refused_in_the_fallback(self) -> None:
        with mock.patch.object(pack, "_pyyaml", None):
            with self.assertRaises(pack.YamlError):
                pack.load_yaml_text("owner: one\nowner: two\n")

    def test_yaml_1_1_booleans_are_read_in_the_fallback(self) -> None:
        with mock.patch.object(pack, "_pyyaml", None):
            data = pack.load_yaml_text(
                "a: yes\nb: no\nc: on\nd: off\n"
            )
        self.assertEqual(
            data, {"a": True, "b": False, "c": True, "d": False}
        )


class PackCase(unittest.TestCase):
    """Shared disposable config and project helpers."""

    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="pack-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def run_pack(
        self,
        *arguments: str,
        env: dict[str, str] | None = None,
        cwd: Path | None = None,
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(PACK), *arguments],
            cwd=cwd or REPO_ROOT,
            env=env or clean_env(),
            capture_output=True,
            text=True,
            check=False,
        )

    def config(self, asma: bool = False) -> Path:
        config = self.tmp / "config"
        shutil.copytree(EXAMPLES, config)
        if asma:
            shutil.copy(
                ASMA_FIXTURE, config / "orchestration" / "orchestration.yml"
            )
        return config

    def project(self, config: Path, render: bool = True) -> Path:
        project = self.tmp / "project"
        project.mkdir()
        self.public_docs(project)
        self.router(project)
        if render:
            result = self.run_pack(
                "--config-dir", str(config), "render", str(project)
            )
            self.assertEqual(result.returncode, 0, result.stderr)
        return project

    def public_docs(self, project: Path) -> None:
        docs = project / "docs"
        docs.mkdir()
        for name in PUBLIC_DOCS:
            shutil.copy(REPO_ROOT / "docs" / name, docs / name)
        (docs / "decisions").mkdir()
        shutil.copy(
            REPO_ROOT / "docs" / "decisions" / "README.md",
            docs / "decisions" / "README.md",
        )

    def router(self, project: Path, extra: list[tuple[str, str]] | None = None) -> None:
        rows = [
            ("Workflow", "WORKFLOW.md"),
            ("Glossary", "GLOSSARY.md"),
            ("Principles", "PRINCIPLES.md"),
            ("Decisions", "decisions/README.md"),
            *(extra or []),
        ]
        lines = [
            "# Project documentation index",
            "",
            "| Document | Summary | Status | When to load |",
            "| --- | --- | --- | --- |",
        ]
        lines += [
            f"| [{title}]({path}) | {title} | Current | When needed |"
            for title, path in rows
        ]
        (project / "docs" / "index.md").write_text(
            "\n".join(lines) + "\n", encoding="utf-8"
        )

    def write_doc(self, project: Path, relative: str, text: str) -> Path:
        path = project / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def plan_text(
        self,
        *,
        status: str = "Draft",
        tracker: str = "EXM-1",
        epic: str = "EXM-100",
        hold: str = "",
    ) -> str:
        graph = {
            "schema_version": 1,
            "epic": {"jira": epic, "kontor_id": None},
            "tasks": [
                {
                    "id": "TASK-001",
                    "title": "Do the thing",
                    "phase": "Phase 1",
                    "jira": tracker,
                    "kontor_id": None,
                    "depends_on": [],
                }
            ],
            "external_fences": [],
        }
        text = (
            "---\n"
            f"status: {status}\n"
            "orchestration: paseo\n"
            "---\n\n"
            "# A disposable plan\n\n"
            "## 2. Implementation Steps\n\n"
            "| Task | Tracker key | Description | Closure condition | Completed | Date |\n"
            "| --- | --- | --- | --- | --- | --- |\n"
            f"| TASK-001 | {tracker} | Do the thing | It is done | | |\n\n"
            "### 2.5 Dependency Graph and Waves\n\n"
            "```plan-graph\n"
            + json.dumps(graph, indent=2)
            + "\n```\n\n"
            "<!-- plan-graph:derived:start -->\n"
            "<!-- plan-graph:derived:end -->\n"
        )
        if hold:
            text += "\n## 8. Holds\n\n```yaml\n" + hold + "\n```\n"
        return text

    def add_plan(
        self, project: Path, config: Path, *, plan: str, relative: str
    ) -> Path:
        path = self.write_doc(project, relative, plan)
        sync = self.run_pack(
            "--config-dir",
            str(config),
            "sync",
            str(path),
            env=clean_env(KONTOR_CONFIG_DIR=str(config)),
        )
        self.assertEqual(sync.returncode, 0, sync.stderr)
        return path


class RenderTests(PackCase):
    def test_render_into_an_empty_project_succeeds(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        skill = project / ".github" / "skills" / "epic-orchestration" / "SKILL.md"
        self.assertTrue(skill.is_file())
        rendered = skill.read_text(encoding="utf-8")
        self.assertNotIn("{{", rendered)
        self.assertIn("example worktree add", rendered)
        self.assertEqual(
            (project / ".agents" / "skills" / "epic-orchestration" / "SKILL.md")
            .read_bytes(),
            skill.read_bytes(),
        )
        for resource in (
            "docs/WORKFLOW.md",
            "docs/GLOSSARY.md",
            "workflow/templates/plan.md",
            "config/examples/paseo-supervision.yml",
        ):
            self.assertTrue(
                (project / ".github" / "skills" / ".pack" / resource).is_file(),
                resource,
            )

    def test_render_accepts_the_asma_team_yaml_shapes(self) -> None:
        config = self.config()
        (config / "packs" / "team.yml").write_text(
            "seats:\n"
            "- role_slot_id: lsa\n"
            "  role_code: LSA\n"
            "tags:\n"
            "  [one, two]\n",
            encoding="utf-8",
        )
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_render_into_the_asma_fixture_succeeds(self) -> None:
        config = self.config(asma=True)
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        rendered = (
            project
            / ".github"
            / "skills"
            / "long-task-commit-checkpoints"
            / "SKILL.md"
        ).read_text(encoding="utf-8")
        self.assertNotIn("{{", rendered)
        self.assertIn("/ASMA-<number>-<slug>", rendered)

    def test_an_unfilled_placeholder_is_refused(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace("  owner: Platform Owner\n", ""), encoding="utf-8"
        )
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("owner", result.stderr)

    def test_a_null_parameter_is_refused(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace("    command: example\n", "    command:\n"),
            encoding="utf-8",
        )
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("project.cli.command", result.stderr)
        checked = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("project.cli.command", checked.stderr)

    def test_an_unclosed_flow_sequence_is_refused(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace(
                "  owner: Platform Owner\n", "  owner: [Platform Owner\n"
            ),
            encoding="utf-8",
        )
        project = self.tmp / "project"
        project.mkdir()
        render = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertNotEqual(render.returncode, 0)
        self.assertIn("unclosed flow sequence", render.stderr)
        checked = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("unclosed flow sequence", checked.stderr)

    def test_rendering_is_deterministic(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        project.mkdir()
        self.assertEqual(
            self.run_pack(
                "--config-dir", str(config), "render", str(project)
            ).returncode,
            0,
        )
        first = {
            path.relative_to(project).as_posix(): path.read_bytes()
            for path in sorted(project.rglob("*"))
            if path.is_file()
        }
        self.assertEqual(
            self.run_pack(
                "--config-dir", str(config), "render", str(project)
            ).returncode,
            0,
        )
        second = {
            path.relative_to(project).as_posix(): path.read_bytes()
            for path in sorted(project.rglob("*"))
            if path.is_file()
        }
        self.assertEqual(first, second)

    def test_the_project_mirror_is_invoked_when_present(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        project.mkdir()
        marker = project / "mirror-ran.txt"
        script = project / "_tools" / "scripts" / "check-codex-agent-config.py"
        script.parent.mkdir(parents=True)
        script.write_text(
            "from pathlib import Path\n"
            "Path('mirror-ran.txt').write_text('yes', encoding='utf-8')\n",
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir", str(config), "render", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(marker.is_file())
        self.assertIn("project mirror passed", result.stdout)
        self.assertTrue(
            (project / ".agents" / "skills" / ".pack").is_dir()
        )
        self.assertFalse(
            (project / ".agents" / "skills" / "epic-orchestration").exists()
        )


class ConfigSelectionTests(PackCase):
    def test_an_absent_selection_is_refused_by_name(self) -> None:
        caller = self.tmp / "caller"
        shutil.copytree(EXAMPLES, caller)
        (caller / "project" / "docs").mkdir(parents=True)
        result = self.run_pack("check", "project", cwd=caller)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--config-dir", result.stderr)
        self.assertIn("KONTOR_CONFIG_DIR", result.stderr)

    def test_a_missing_folder_is_refused_by_name(self) -> None:
        result = self.run_pack(
            "--config-dir", str(self.tmp / "missing"), "check", str(self.tmp)
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not exist", result.stderr)

    def test_the_flag_wins_over_the_environment(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir",
            str(config),
            "render",
            str(project),
            env=clean_env(
                KONTOR_CONFIG_DIR=str(self.tmp / "does-not-exist")
            ),
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_resolution_does_not_depend_on_the_caller_directory(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        project.mkdir()
        first = self.run_pack(
            "--config-dir",
            "config",
            "render",
            "project",
            cwd=self.tmp,
            env=clean_env(KONTOR_CONFIG_DIR=str(self.tmp)),
        )
        self.assertEqual(first.returncode, 0, first.stderr)
        from_here = self.run_pack(
            "--config-dir",
            str(config),
            "render",
            str(project),
            cwd=REPO_ROOT,
        )
        self.assertEqual(from_here.returncode, 0, from_here.stderr)
        self.assertIn(str(config.resolve()), from_here.stdout)


class CheckTests(PackCase):
    def test_check_passes_on_an_empty_project(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        (project / "docs").mkdir(parents=True)
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_check_passes_on_a_rendered_project(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_schema_project_required_drives_missing_keys(self) -> None:
        config = self.config()
        schema = config / "orchestration" / "orchestration.schema.yml"
        text = schema.read_text(encoding="utf-8")
        schema.write_text(
            text.replace(
                "project:\n  required:\n    - key\n",
                "project:\n  required:\n    - custom_required\n    - key\n",
            ),
            encoding="utf-8",
        )
        project = self.tmp / "project"
        (project / "docs").mkdir(parents=True)
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("custom_required", result.stderr)

    def test_a_missing_parameter_fails_check(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace("  owner: Platform Owner\n", ""), encoding="utf-8"
        )
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("owner", result.stderr)

    def test_a_link_to_an_explicit_html_anchor_passes(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(
            project,
            "docs/guide.md",
            '# Guide\n\n<a id="manual-anchor"></a>\n',
        )
        self.write_doc(
            project,
            "docs/other.md",
            "# Other\n\n[guide](guide.md#manual-anchor)\n",
        )
        self.router(
            project,
            [("Guide", "guide.md"), ("Other", "other.md")],
        )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_broken_anchor_fails_check(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(
            project,
            "docs/guide.md",
            "# Guide\n\nSee [workflow](WORKFLOW.md#not-a-heading).\n",
        )
        self.router(project, [("Guide", "guide.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("broken anchor", result.stderr)
        self.assertIn("guide.md", result.stderr)

    def test_an_unrouted_document_fails_check(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(project, "docs/extra.md", "# Extra\n")
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("unrouted document", result.stderr)
        self.assertIn("extra.md", result.stderr)

    def test_rendered_skill_same_file_anchor_is_checked(self) -> None:
        config = self.config()
        project = self.project(config)
        for base in (".github", ".agents"):
            path = project / base / "skills" / "epic-orchestration" / "SKILL.md"
            path.write_text(
                path.read_text(encoding="utf-8")
                + "\n[missing heading](#nonexistent-heading)\n",
                encoding="utf-8",
            )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("broken anchor", result.stderr)
        self.assertIn("#nonexistent-heading", result.stderr)

    def test_rendered_skill_links_are_checked(self) -> None:
        config = self.config()
        project = self.project(config)
        for base in (".github", ".agents"):
            path = project / base / "skills" / "epic-orchestration" / "SKILL.md"
            path.write_text(
                path.read_text(encoding="utf-8")
                + "\n[missing](missing.md)\n",
                encoding="utf-8",
            )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("broken link", result.stderr)
        self.assertIn("missing.md", result.stderr)

    def test_an_invalid_documentation_path_does_not_disable_checks(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace(
                "  documentation_roots: [project/docs]",
                "  documentation_roots: [project/docs, ../outside]",
            ),
            encoding="utf-8",
        )
        project = self.project(config)
        self.write_doc(project, "docs/extra.md", "# Extra\n")
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("escapes the project", result.stderr)
        self.assertIn("unrouted document", result.stderr)
        self.assertIn("extra.md", result.stderr)

    def test_an_undeclared_resource_root_is_refused(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace(
                "  documentation_roots: [project/docs]",
                "  documentation_roots: [unknown/docs]",
            ),
            encoding="utf-8",
        )
        project = self.project(config)
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("undeclared resource root", result.stderr)

    def test_a_resource_root_must_be_listed_in_forge_repositories(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        orchestration.write_text(
            text.replace(
                "    project: example-org/example-project\n",
                "    project: example-org/unlisted\n",
            ),
            encoding="utf-8",
        )
        project = self.project(config)
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "does not name a project.forge.repositories entry",
            result.stderr,
        )

    def test_a_missing_documentation_root_is_refused(self) -> None:
        config = self.config()
        project = self.tmp / "project"
        project.mkdir()
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "documentation_roots path does not exist: project/docs",
            result.stderr,
        )

    def test_yaml_boolean_always_apply_is_enforced(self) -> None:
        config = self.config()
        project = self.project(config)
        rule = project / ".github" / "rules" / "wide.mdc"
        rule.parent.mkdir(parents=True, exist_ok=True)
        rule.write_text(
            "---\n"
            "description: wide rule\n"
            "globs: '**'\n"
            "alwaysApply: true\n"
            "---\n"
            + ("x" * 17000)
            + "\n",
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("Cursor unconditional instructions", result.stderr)

    def test_nested_routers_are_allowed(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(project, "docs/sub/guide.md", "# Guide\n")
        self.write_doc(
            project,
            "docs/sub/index.md",
            "# Sub documentation index\n\n"
            "| Document | Summary | Status | When to load |\n"
            "| --- | --- | --- | --- |\n"
            "| [Guide](guide.md) | Guide | Current | Always |\n",
        )
        self.router(project, [("Sub index", "sub/index.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_nested_router_must_route_its_documents(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(project, "docs/sub/guide.md", "# Guide\n")
        self.write_doc(
            project,
            "docs/sub/index.md",
            "# Sub documentation index\n\n"
            "| Document | Summary | Status | When to load |\n"
            "| --- | --- | --- | --- |\n"
            "| [Other](other.md) | Other | Current | Always |\n",
        )
        self.write_doc(project, "docs/sub/other.md", "# Other\n")
        self.router(project, [("Sub index", "sub/index.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("unrouted document", result.stderr)
        self.assertIn("guide.md", result.stderr)

    def test_a_json_escaped_home_path_fails_public_lint(self) -> None:
        config = self.config()
        project = self.project(config)
        escaped = "\\" + "/" + "Users" + "\\" + "/" + "jdoe"
        self.write_doc(
            project,
            "docs/notes.md",
            '# Notes\n\n{"path": "' + escaped + '"}\n',
        )
        self.router(project, [("Notes", "notes.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("home_path", result.stderr)

    def test_a_synthetic_evidence_receipt_is_refused(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(
            project,
            "docs/evidence/EXM-1/receipts/run.md",
            "# Run receipt\n\nresult: synthetic\n",
        )
        self.router(
            project,
            [("Evidence receipt", "evidence/EXM-1/receipts/run.md")],
        )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("run_receipt", result.stderr)
        self.assertIn(
            "project/docs/evidence/EXM-1/receipts/run.md",
            result.stderr,
        )

    def test_explicit_resource_roots_keep_repositories_separate(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        text = text.replace(
            "    project: example-org/example-project\n",
            "    one: example-org/one\n    two: example-org/two\n",
        )
        text = text.replace(
            "      - example-org/example-project\n",
            "      - example-org/one\n      - example-org/two\n",
        )
        text = text.replace(
            "decision_records_root: project/docs/decisions/",
            "decision_records_root: one/docs/decisions/",
        )
        text = text.replace(
            "  documentation_roots: [project/docs]",
            "  documentation_roots: [one/docs, two/docs]",
        )
        text = text.replace(
            "  gap_ledger: project/docs/GAPS.md",
            "  gap_ledger: one/docs/GAPS.md",
        )
        text = text.replace(
            "  glossary: project/docs/GLOSSARY.md",
            "  glossary: one/docs/GLOSSARY.md",
        )
        orchestration.write_text(text, encoding="utf-8")

        one = self.tmp / "one"
        two = self.tmp / "two"
        candidate = self.tmp / "candidate"
        (one / "docs").mkdir(parents=True)
        (two / "docs").mkdir(parents=True)
        candidate.mkdir()
        (two / "docs" / "index.md").write_text(
            "# Docs\n\n"
            "| Document | Summary | Status | When to load |\n"
            "| --- | --- | --- | --- |\n"
            "| [Guide](guide.md) | Guide | Current | Always |\n",
            encoding="utf-8",
        )
        (two / "docs" / "guide.md").write_text(
            "# Guide\n\n[missing](missing.md)\n", encoding="utf-8"
        )

        unchecked = self.run_pack(
            "--config-dir", str(config), "check", str(candidate)
        )
        self.assertEqual(unchecked.returncode, 1)
        self.assertIn("cannot be checked without --resource-root", unchecked.stderr)
        self.assertIn("[NOTE] resource root one", unchecked.stdout)
        self.assertIn("[NOTE] resource root two", unchecked.stdout)

        checked = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(candidate),
            "--resource-root",
            f"one={one}",
            "--resource-root",
            f"two={two}",
        )
        self.assertEqual(checked.returncode, 1)
        self.assertIn("two/docs/guide.md", checked.stderr)
        self.assertIn("broken link", checked.stderr)
        self.assertNotIn("candidate/docs", checked.stderr)

        (one / "docs" / "plans").mkdir()
        (one / "docs" / "plans" / "plan-one.md").write_text(
            "# Plan one\n", encoding="utf-8"
        )
        sibling_plan = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(candidate),
            "--resource-root",
            f"one={one}",
            "--resource-root",
            f"two={two}",
            "--plan-graphs",
            "marker",
            "--no-links",
            "--no-routed-once",
            "--no-public-lint",
        )
        self.assertEqual(sibling_plan.returncode, 0, sibling_plan.stderr)

        strict_plan = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(candidate),
            "--resource-root",
            f"one={one}",
            "--resource-root",
            f"two={two}",
            "--plan-graphs",
            "all",
            "--no-links",
            "--no-routed-once",
            "--no-public-lint",
        )
        self.assertEqual(strict_plan.returncode, 1)
        self.assertIn(
            "one/docs/plans/plan-one.md: current plan requires",
            strict_plan.stderr,
        )
        self.assertNotIn(str(one), strict_plan.stderr)

        held = one / "docs" / "plans" / "plan-held.md"
        held.write_text(
            "---\n"
            "status: On Hold\n"
            "orchestration: paseo\n"
            "hold:\n"
            "  reason: waiting_on_dependency\n"
            "  owner: Platform Owner\n"
            "  lifts_when: the dependency closes\n"
            "  review_by: 2020-01-01\n"
            "---\n\n"
            "# Held plan\n",
            encoding="utf-8",
        )
        held_result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(candidate),
            "--resource-root",
            f"one={one}",
            "--resource-root",
            f"two={two}",
            "--plan-graphs",
            "marker",
            "--no-links",
            "--no-routed-once",
            "--no-public-lint",
        )
        self.assertEqual(held_result.returncode, 1)
        self.assertIn(
            "one/docs/plans/plan-held.md: expired hold",
            held_result.stderr,
        )
        self.assertNotIn(str(one), held_result.stderr)

        (two / "docs" / "README.md").write_text(
            "# Second documentation index\n", encoding="utf-8"
        )
        ambiguous_routers = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(candidate),
            "--resource-root",
            f"one={one}",
            "--resource-root",
            f"two={two}",
            "--plan-graphs",
            "marker",
            "--no-links",
            "--no-public-lint",
        )
        self.assertEqual(ambiguous_routers.returncode, 1)
        self.assertIn(
            "more than one documentation router in two/docs",
            ambiguous_routers.stderr,
        )
        self.assertNotIn(str(two), ambiguous_routers.stderr)

    def test_an_undeclared_resource_override_is_refused(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--resource-root",
            f"other={project}",
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("undeclared resource root override", result.stderr)

    def test_non_pack_skill_placeholders_are_ignored(self) -> None:
        config = self.config()
        project = self.project(config)
        text = (
            "---\n"
            "name: asma-only\n"
            "description: A project-owned skill with its own template syntax.\n"
            "---\n\n"
            "# Project skill\n\n{{ project-owned-value }}\n"
        )
        for base in (".github", ".agents"):
            directory = project / base / "skills" / "asma-only"
            directory.mkdir(parents=True)
            (directory / "SKILL.md").write_text(text, encoding="utf-8")
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_rendered_link_that_leaves_the_skill_tree_is_refused(self) -> None:
        config = self.config()
        project = self.project(config)
        for base in (".github", ".agents"):
            path = project / base / "skills" / "epic-orchestration" / "SKILL.md"
            path.write_text(
                path.read_text(encoding="utf-8")
                + "\n[outside](../../../docs/WORKFLOW.md)\n",
                encoding="utf-8",
            )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("relative link leaves the skill tree", result.stderr)

    def test_missing_rendered_resource_mirror_is_refused(self) -> None:
        config = self.config()
        project = self.project(config)
        shutil.rmtree(project / ".agents" / "skills" / ".pack")
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing .agents mirror", result.stderr)

    def test_rendered_resource_mirror_content_is_checked(self) -> None:
        config = self.config()
        project = self.project(config)
        target = (
            project
            / ".agents"
            / "skills"
            / ".pack"
            / "docs"
            / "WORKFLOW.md"
        )
        target.write_text("changed\n", encoding="utf-8")
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("differ from the .agents mirror", result.stderr)

    def test_a_rendered_pack_placeholder_is_refused(self) -> None:
        config = self.config()
        project = self.project(config)
        for base in (".github", ".agents"):
            path = project / base / "skills" / "epic-orchestration" / "SKILL.md"
            path.write_text(
                path.read_text(encoding="utf-8")
                + "\n{{ project.owner }}\n",
                encoding="utf-8",
            )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("unfilled placeholder", result.stderr)

    def test_plan_graphs_marker_allows_a_current_plan_without_a_graph(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(
            project,
            "docs/plans/plan-no-graph.md",
            "---\nstatus: Draft\norchestration: paseo\n---\n\n"
            "# No graph\n\n## 2. Implementation Steps\n\n"
            "| Task | Tracker key | Description | Closure condition | "
            "Completed | Date |\n"
            "| --- | --- | --- | --- | --- | --- |\n"
            "| TASK-001 | EXM-1 | Do | Done | | |\n",
        )
        self.router(project, [("No graph", "plans/plan-no-graph.md")])
        strict = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(strict.returncode, 1)
        self.assertIn("current plan requires", strict.stderr)
        marker = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--plan-graphs",
            "marker",
        )
        self.assertEqual(marker.returncode, 0, marker.stderr)

    def test_plan_graphs_changed_checks_only_changed_plans(self) -> None:
        config = self.config()
        project = self.project(config)
        plan = self.add_plan(
            project,
            config,
            plan=self.plan_text(),
            relative="docs/plans/plan-changed.md",
        )
        self.router(project, [("Changed plan", "plans/plan-changed.md")])
        subprocess.run(["git", "init"], cwd=project, check=True,
                       capture_output=True)
        subprocess.run(
            ["git", "config", "user.email", "test@example.invalid"],
            cwd=project,
            check=True,
            capture_output=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Test"],
            cwd=project,
            check=True,
            capture_output=True,
        )
        subprocess.run(["git", "add", "-A"], cwd=project, check=True,
                       capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "base"],
            cwd=project,
            check=True,
            capture_output=True,
        )
        plan.write_text(
            "---\nstatus: Draft\norchestration: paseo\n---\n\n"
            "# Changed plan\n",
            encoding="utf-8",
        )
        changed = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--plan-graphs",
            "changed",
            "--base",
            "HEAD",
        )
        self.assertEqual(changed.returncode, 1)
        self.assertIn("current plan requires", changed.stderr)
        marker = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--plan-graphs",
            "marker",
        )
        self.assertEqual(marker.returncode, 0, marker.stderr)

    def test_plan_graphs_changed_skips_unchanged_graph_less_plans(self) -> None:
        config = self.config()
        project = self.project(config)
        unchanged = self.tmp / "project" / "docs" / "plans" / "plan-stable.md"
        changed = self.tmp / "project" / "docs" / "plans" / "plan-moved.md"
        graph_less = (
            "---\nstatus: Draft\norchestration: paseo\n---\n\n"
            "# Plan\n"
        )
        for path in (unchanged, changed):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(graph_less, encoding="utf-8")
        self.router(
            project,
            [
                ("Stable plan", "plans/plan-stable.md"),
                ("Moved plan", "plans/plan-moved.md"),
            ],
        )
        subprocess.run(["git", "init"], cwd=project, check=True,
                       capture_output=True)
        subprocess.run(
            ["git", "config", "user.email", "test@example.invalid"],
            cwd=project,
            check=True,
            capture_output=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Test"],
            cwd=project,
            check=True,
            capture_output=True,
        )
        subprocess.run(["git", "add", "-A"], cwd=project, check=True,
                       capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "base"],
            cwd=project,
            check=True,
            capture_output=True,
        )
        changed.write_text(
            "---\nstatus: Completed\norchestration: paseo\n---\n\n"
            "# Plan\n",
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--plan-graphs",
            "changed",
            "--base",
            "HEAD",
            "--no-links",
            "--no-routed-once",
            "--no-public-lint",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("plan-stable.md", result.stderr)

    def test_plan_graphs_changed_requires_a_base(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--plan-graphs",
            "changed",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires --base", result.stderr)

    def test_no_routed_once_and_no_public_lint_are_explicit_opt_outs(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(project, "docs/extra.md", "# Extra\n")
        escaped = "\\" + "/" + "Users" + "\\" + "/" + "jdoe"
        self.write_doc(
            project,
            "docs/notes.md",
            '# Notes\n\n[broken](missing.md)\n\n'
            '{"path": "' + escaped + '"}\n',
        )
        self.router(project, [("Notes", "notes.md")])
        refused = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(refused.returncode, 1)
        self.assertIn("unrouted document", refused.stderr)
        self.assertIn("broken link", refused.stderr)
        self.assertIn("home_path", refused.stderr)
        accepted = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--no-routed-once",
            "--no-public-lint",
            "--no-links",
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)

    def test_routing_requires_one_claude_import(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(project, "AGENTS.md", "# Agents\n")
        self.write_doc(project, "CLAUDE.md", "# Claude\n")
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("must import AGENTS.md exactly once", result.stderr)

    def test_malformed_frontmatter_without_a_hold_is_ignored(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(
            project,
            "docs/plans/plan-legacy.md",
            "---\n"
            "> **Date:** 2026-07-13 13:45\n"
            "---\n\n"
            "# Legacy plan\n",
        )
        self.router(project, [("Legacy", "plans/plan-legacy.md")])
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            "--plan-graphs",
            "marker",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_an_expired_hold_fails_check(self) -> None:
        config = self.config()
        project = self.project(config)
        yesterday = (date.today() - timedelta(days=1)).isoformat()
        hold = (
            "hold:\n"
            "  reason: waiting_on_dependency\n"
            "  owner: Platform Owner\n"
            "  lifts_when: The dependency lands\n"
            f"  review_by: '{yesterday}'"
        )
        self.add_plan(
            project,
            config,
            plan=self.plan_text(status="On Hold", hold=hold),
            relative="docs/plans/plan-held.md",
        )
        self.router(project, [("Held plan", "plans/plan-held.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("expired hold", result.stderr)

    def test_a_valid_hold_passes_check(self) -> None:
        config = self.config()
        project = self.project(config)
        review = (date.today() + timedelta(days=7)).isoformat()
        hold = (
            "hold:\n"
            "  reason: waiting_on_dependency\n"
            "  owner: Platform Owner\n"
            "  lifts_when: The dependency lands\n"
            f"  review_by: '{review}'"
        )
        self.add_plan(
            project,
            config,
            plan=self.plan_text(status="On Hold", hold=hold),
            relative="docs/plans/plan-held.md",
        )
        self.router(project, [("Held plan", "plans/plan-held.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_hold_fields_and_review_window_are_checked(self) -> None:
        config = self.config()
        project = self.project(config)
        review = (date.today() + timedelta(days=60)).isoformat()
        hold = (
            "hold:\n"
            "  reason: not_a_reason\n"
            "  owner: ''\n"
            "  lifts_when: ''\n"
            f"  review_by: '{review}'"
        )
        self.add_plan(
            project,
            config,
            plan=self.plan_text(status="On Hold", hold=hold),
            relative="docs/plans/plan-held.md",
        )
        self.router(project, [("Held plan", "plans/plan-held.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("hold reason", result.stderr)
        self.assertIn("hold field owner", result.stderr)
        self.assertIn("hold field lifts_when", result.stderr)
        self.assertIn("more than 30 days ahead", result.stderr)

    def test_mirror_and_metadata_are_checked(self) -> None:
        config = self.config()
        project = self.project(config)
        mirror = (
            project / ".agents" / "skills" / "epic-orchestration" / "SKILL.md"
        )
        mirror.write_text(
            mirror.read_text(encoding="utf-8") + "\nhand edit\n",
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("mirror differs", result.stderr)

    def test_skill_metadata_is_checked(self) -> None:
        config = self.config()
        project = self.project(config)
        skill = (
            project / ".github" / "skills" / "epic-orchestration" / "SKILL.md"
        )
        text = skill.read_text(encoding="utf-8")
        skill.write_text(
            text.replace(
                "name: epic-orchestration", "name: not-the-directory", 1
            ),
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("differs from directory", result.stderr)

    def test_a_configured_but_missing_validator_fails_clearly(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            env=clean_env(
                ASMA_PLAN_GRAPH_VALIDATOR=str(self.tmp / "missing-validator.py")
            ),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ASMA_PLAN_GRAPH_VALIDATOR", result.stderr)
        self.assertIn("refusing to skip", result.stderr)

    def test_ci_does_not_require_an_external_validator(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            env=clean_env(CI="true"),
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_the_plan_graph_uses_the_project_key_pattern(self) -> None:
        config = self.config()
        project = self.project(config)
        plan = self.add_plan(
            project,
            config,
            plan=self.plan_text(),
            relative="docs/plans/plan-wrong-key.md",
        )
        text = plan.read_text(encoding="utf-8")
        plan.write_text(
            text.replace("EXM-1", "exm-1").replace("EXM-100", "exm-100"),
            encoding="utf-8",
        )
        self.router(project, [("Wrong key", "plans/plan-wrong-key.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("key_pattern", result.stderr)

    def test_the_plan_graph_uses_a_custom_key_pattern(self) -> None:
        config = self.config()
        orchestration = config / "orchestration" / "orchestration.yml"
        text = orchestration.read_text(encoding="utf-8")
        text = text.replace("  key: EXM\n", "  key: ktr\n")
        text = text.replace(
            "    key_pattern: '^[A-Z][A-Z0-9]{1,9}-[0-9]+$'",
            "    key_pattern: '^[a-z]+-[0-9]+$'",
        )
        orchestration.write_text(text, encoding="utf-8")
        project = self.project(config)
        self.add_plan(
            project,
            config,
            plan=self.plan_text(tracker="ktr-1", epic="ktr-100"),
            relative="docs/plans/plan-lowercase.md",
        )
        self.router(project, [("Lowercase key", "plans/plan-lowercase.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_the_external_validator_is_invoked(self) -> None:
        config = self.config()
        project = self.project(config)
        self.add_plan(
            project,
            config,
            plan=self.plan_text(),
            relative="docs/plans/plan-example.md",
        )
        self.router(project, [("Example plan", "plans/plan-example.md")])
        marker = self.tmp / "validator-ran.txt"
        validator = self.tmp / "validator.py"
        validator.write_text(
            "from pathlib import Path\n"
            f"Path({str(marker)!r}).write_text('ran', encoding='utf-8')\n",
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            env=clean_env(KONTOR_PLAN_GRAPH_VALIDATOR=str(validator)),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(marker.is_file())

    def test_an_external_validator_is_run_when_configured(self) -> None:
        config = self.config()
        project = self.project(config)
        self.add_plan(
            project,
            config,
            plan=self.plan_text(),
            relative="docs/plans/plan-example.md",
        )
        self.router(project, [("Example plan", "plans/plan-example.md")])
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            env=clean_env(ASMA_PLAN_GRAPH_VALIDATOR=str(PACK)),
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_neutral_validator_name_is_supported(self) -> None:
        config = self.config()
        project = self.project(config)
        self.add_plan(
            project,
            config,
            plan=self.plan_text(),
            relative="docs/plans/plan-example.md",
        )
        self.router(project, [("Example plan", "plans/plan-example.md")])
        marker = self.tmp / "neutral-validator-ran.txt"
        validator = self.tmp / "neutral-validator.py"
        validator.write_text(
            "from pathlib import Path\n"
            f"Path({str(marker)!r}).write_text('ran', encoding='utf-8')\n",
            encoding="utf-8",
        )
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            env=clean_env(KONTOR_PLAN_GRAPH_VALIDATOR=str(validator)),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(marker.is_file())

    def test_the_plan_graph_is_mandatory_for_a_current_plan(self) -> None:
        config = self.config()
        project = self.project(config)
        self.write_doc(
            project,
            "docs/plans/plan-no-graph.md",
            "---\nstatus: Draft\norchestration: paseo\n---\n\n"
            "# No graph\n\n## 2. Implementation Steps\n\n"
            "| Task | Tracker key | Description | Closure condition | "
            "Completed | Date |\n"
            "| --- | --- | --- | --- | --- | --- |\n"
            "| TASK-001 | EXM-1 | Do | Done | | |\n",
        )
        self.router(project, [("No graph", "plans/plan-no-graph.md")])
        result = self.run_pack(
            "--config-dir", str(config), "check", str(project)
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("plan-graph", result.stderr)


class AdapterTests(PackCase):
    def test_install_adapters_requires_an_explicit_target(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack("install-adapters", str(project), "--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--target-root", result.stderr)

    def test_install_adapters_links_each_canonical_skill(self) -> None:
        config = self.config()
        project = self.project(config)
        target = self.tmp / "target"
        target.mkdir()
        before = self.run_pack(
            "install-adapters",
            str(project),
            "--target-root",
            str(target),
            "--check",
        )
        self.assertNotEqual(before.returncode, 0)
        applied = self.run_pack(
            "install-adapters",
            str(project),
            "--target-root",
            str(target),
            "--apply",
        )
        self.assertEqual(applied.returncode, 0, applied.stderr)
        source = project / ".github" / "skills" / "epic-orchestration"
        for base in (".agents", ".claude"):
            link = target / base / "skills" / "epic-orchestration"
            self.assertTrue(link.is_symlink())
            self.assertEqual(link.resolve(), source.resolve())
        after = self.run_pack(
            "install-adapters",
            str(project),
            "--target-root",
            str(target),
            "--check",
        )
        self.assertEqual(after.returncode, 0, after.stderr)


if __name__ == "__main__":
    unittest.main()
