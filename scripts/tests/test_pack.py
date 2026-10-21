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

REPO_ROOT = Path(__file__).resolve().parents[2]
PACK = REPO_ROOT / "scripts" / "pack.py"
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
        self.assertFalse((project / ".agents").exists())


class ConfigSelectionTests(PackCase):
    def test_an_absent_selection_is_refused_by_name(self) -> None:
        result = self.run_pack("check", str(self.tmp))
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
        project.mkdir()
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

    def test_ci_requires_the_validator(self) -> None:
        config = self.config()
        project = self.project(config)
        result = self.run_pack(
            "--config-dir",
            str(config),
            "check",
            str(project),
            env=clean_env(CI="true"),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must be set in CI", result.stderr)

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
            env=clean_env(
                KONTOR_CONFIG_DIR=str(config),
                ASMA_PLAN_GRAPH_VALIDATOR=str(PACK),
            ),
        )
        self.assertEqual(result.returncode, 0, result.stderr)

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
