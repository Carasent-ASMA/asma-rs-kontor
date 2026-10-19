"""Tests for the launchd deploy script and its target guard (KR-05, G-01).

No test here may reach launchctl. Every test hands the script a recording host
that starts no process; the real host is replaced by one that fails the test,
and a ``launchctl`` on PATH records any call that slips past both.

Run from the repository root:

    PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -v
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "kontor-deploy.py"

_spec = importlib.util.spec_from_file_location("kontor_deploy", SCRIPT)
deploy = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = deploy
_spec.loader.exec_module(deploy)

PRODUCTION_LABEL = "com.asma.kontor.daemon"
DISPOSABLE_LABEL = "com.asma.kontor.test.daemon"
REAL_UID = os.getuid()
REAL_GUI_DOMAIN = f"gui/{REAL_UID}"
DISPOSABLE_UID = 4242 if REAL_UID != 4242 else 4243
DISPOSABLE_DOMAIN = f"gui/{DISPOSABLE_UID}"
REALM_ID = "01a00000-0000-7000-8000-000000000001"

_tripwire: tempfile.TemporaryDirectory | None = None
_real_host = deploy.real_host
_path = os.environ.get("PATH", "")


def setUpModule() -> None:
    global _tripwire
    _tripwire = tempfile.TemporaryDirectory(prefix="kontor-deploy-tripwire-")
    stub = Path(_tripwire.name) / "launchctl"
    stub.write_text('#!/bin/sh\necho "$@" >> "$0.calls"\nexit 97\n')
    stub.chmod(0o755)
    os.environ["PATH"] = f"{_tripwire.name}{os.pathsep}{_path}"

    def refuse_real_host():
        raise AssertionError("a test reached the real host")

    deploy.real_host = refuse_real_host


def tearDownModule() -> None:
    deploy.real_host = _real_host
    os.environ["PATH"] = _path
    calls = Path(_tripwire.name) / "launchctl.calls"
    reached = calls.read_text() if calls.exists() else ""
    _tripwire.cleanup()
    if reached:
        raise AssertionError(f"a test reached launchctl: {reached}")


def done(code: int = 0, stdout: str = "") -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(args=[], returncode=code, stdout=stdout, stderr="")


class Recorder:
    """Records every process the script asks for and starts none of them."""

    def __init__(self, answer=None) -> None:
        self.calls: list[list[str]] = []
        self.answer = answer

    def run(self, argv):
        self.calls.append(list(argv))
        if self.answer is None:
            return done(1) if list(argv)[:2] == ["launchctl", "print"] else done()
        return self.answer(list(argv))

    def launchctl(self) -> list[list[str]]:
        return [
            call
            for call in self.calls
            if call[0] == "launchctl" and call[1:2] != ["print"]
        ]

    def prints(self) -> list[list[str]]:
        return [call for call in self.calls if call[:2] == ["launchctl", "print"]]


class World:
    """A disposable install: a home, binaries, a state root and candidates."""

    def __init__(self, root: Path) -> None:
        self.home = root / "home"
        self.agents = self.home / "Library" / "LaunchAgents"
        self.bin = root / "bin"
        self.state = root / "state"
        self.candidates = root / "candidates"
        self.backup = root / "backup"
        for directory in (self.agents, self.bin, self.state, self.candidates):
            directory.mkdir(parents=True)
        for name in ("kontor-daemon", "kontor"):
            (self.bin / name).write_bytes(f"old {name}\n".encode())
        (self.bin / "kontor-mcp.oldgen").write_bytes(b"old kontor-mcp\n")
        (self.bin / "kontor-mcp").symlink_to(self.bin / "kontor-mcp.oldgen")
        for name in deploy.BINARIES:
            (self.candidates / name).write_bytes(f"new {name}\n".encode())

    def plist(
        self,
        label: str,
        path: Path | None = None,
        daemon: Path | None = None,
        state_root: Path | str | None = None,
        port: int | str | None = 47717,
        working_directory: Path | str | None = None,
    ) -> Path:
        path = path or (self.agents / f"{label}.plist")
        program = [
            str(daemon or (self.bin / "kontor-daemon")),
            "--state-root",
            str(state_root if state_root is not None else self.state),
        ]
        if port is not None:
            program.extend(["--port", str(port)])
        document = {"Label": label, "ProgramArguments": program}
        if working_directory is not None:
            document["WorkingDirectory"] = str(working_directory)
        path.write_bytes(plistlib.dumps(document))
        return path

    def host(
        self,
        recorder: Recorder,
        uid: int,
        homes: dict[int, Path] | None = None,
        wait: float = deploy.DEFAULT_WAIT,
    ) -> deploy.Host:
        return deploy.Host(
            uid=uid,
            home=self.home,
            run=recorder.run,
            sleep=lambda seconds: None,
            homes=homes,
            wait=wait,
        )

    def installed(self) -> dict[str, object]:
        """Every file and link under bin, by content or target."""
        return {
            path.name: os.readlink(path) if path.is_symlink() else path.read_bytes()
            for path in sorted(self.bin.iterdir())
        }


def run_main(argv: list[str], host) -> int:
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return deploy.main(argv, host)


def command_argv(command: str, world: World) -> list[str]:
    if command != "replace":
        return [command]
    return [
        "replace",
        "--candidates", str(world.candidates),
        "--backup-dir", str(world.backup),
        "--realm-id", REALM_ID,
    ]


class WorldTest(unittest.TestCase):
    def setUp(self) -> None:
        self._directory = tempfile.TemporaryDirectory(prefix="kontor-deploy-test-")
        self.addCleanup(self._directory.cleanup)
        self.world = World(Path(self._directory.name))


class ProductionJobGuardTest(WorldTest):
    def test_production_label_in_real_gui_domain_is_refused_before_launchctl(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL)
        stray = self.world.plist(PRODUCTION_LABEL, self.world.state / "job.plist")
        before = self.world.installed()
        for command in ("bootout", "bootstrap", "replace"):
            for plist in (canonical, stray):
                with self.subTest(command=command, plist=plist.name):
                    recorder = Recorder()
                    argv = [
                        "--label", PRODUCTION_LABEL,
                        "--domain", REAL_GUI_DOMAIN,
                        "--plist", str(plist),
                        *command_argv(command, self.world),
                    ]
                    code = run_main(argv, self.world.host(recorder, REAL_UID))
                    self.assertEqual(recorder.calls, [])
                    self.assertEqual(code, deploy.EXIT_REFUSED)
                    self.assertEqual(self.world.installed(), before)
                    self.assertFalse(self.world.backup.exists())

    def test_the_refusal_names_the_undeclared_production_job(self) -> None:
        recorder = Recorder()
        with self.assertRaises(deploy.Refused) as refusal:
            deploy.resolve_job(
                PRODUCTION_LABEL,
                REAL_GUI_DOMAIN,
                self.world.plist(PRODUCTION_LABEL),
                None,
                self.world.host(recorder, REAL_UID),
                "bootout",
            )
        self.assertEqual(refusal.exception.code, "production_undeclared")
        self.assertEqual(recorder.calls, [])

    def test_a_case_variant_of_the_production_label_is_the_production_label(self) -> None:
        variant = "COM.ASMA.KONTOR.DAEMON"
        recorder = Recorder()
        argv = [
            "--label", variant,
            "--domain", REAL_GUI_DOMAIN,
            "--plist", str(self.world.plist(variant)),
            "bootout",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, REAL_UID)), deploy.EXIT_REFUSED)
        self.assertEqual(recorder.calls, [])

    def test_a_plist_registering_the_production_label_is_refused_under_another_label(self) -> None:
        # The G-01 shape: a temporary plist that launchd registers as production.
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(PRODUCTION_LABEL, self.world.state / "job.plist")),
            "bootstrap",
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), deploy.EXIT_REFUSED
        )
        self.assertEqual(recorder.calls, [])

    def test_a_declared_production_deploy_refuses_any_plist_but_the_canonical_one(self) -> None:
        self.world.plist(PRODUCTION_LABEL)
        recorder = Recorder()
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", REAL_GUI_DOMAIN,
            "--plist", str(self.world.plist(PRODUCTION_LABEL, self.world.state / "job.plist")),
            "--production-deploy", "ASMA-8506",
            "bootstrap",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, REAL_UID)), deploy.EXIT_REFUSED)
        self.assertEqual(recorder.calls, [])

    def test_a_declared_production_deploy_refuses_any_other_domain(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL)
        for domain in (f"user/{REAL_UID}", f"gui/{REAL_UID + 1}"):
            with self.subTest(domain=domain):
                recorder = Recorder()
                argv = [
                    "--label", PRODUCTION_LABEL,
                    "--domain", domain,
                    "--plist", str(canonical),
                    "--production-deploy", "ASMA-8506",
                    "bootout",
                ]
                code = run_main(argv, self.world.host(recorder, REAL_UID))
                self.assertEqual(code, deploy.EXIT_REFUSED)
                self.assertEqual(recorder.calls, [])

    def test_a_production_deploy_is_declared_by_a_jira_key(self) -> None:
        recorder = Recorder()
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(PRODUCTION_LABEL)),
            "--production-deploy", "yes",
            "bootout",
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), deploy.EXIT_REFUSED
        )
        self.assertEqual(recorder.calls, [])

    def test_a_disposable_label_cannot_address_the_production_install(self) -> None:
        production = self.world.plist(PRODUCTION_LABEL)
        base = plistlib.loads(production.read_bytes())
        base["Label"] = DISPOSABLE_LABEL
        production_daemon = self.world.bin / "kontor-daemon"
        production_state = self.world.state
        other_bin = self.world.home / "other-bin"
        other_state = self.world.home / "other-state"
        other_bin.mkdir()
        other_state.mkdir()
        other_daemon = other_bin / "kontor-daemon"
        other_daemon.write_bytes(b"other daemon\n")
        daemon_alias = self.world.state / "kontor-daemon"
        daemon_alias.symlink_to(production_daemon)
        state_alias = self.world.state / "state-alias"
        state_alias.symlink_to(production_state)

        def arguments(daemon: Path, state_root: Path, port: int = 47718) -> list[str]:
            return [str(daemon), "--state-root", str(state_root), "--port", str(port)]

        cases = {
            "copied production plist": base,
            "production daemon only": {
                **base,
                "ProgramArguments": arguments(production_daemon, other_state),
            },
            "production state root only": {
                **base,
                "ProgramArguments": arguments(other_daemon, production_state),
            },
            "symlinked daemon only": {
                **base,
                "ProgramArguments": arguments(daemon_alias, other_state),
            },
            "symlinked state root only": {
                **base,
                "ProgramArguments": arguments(other_daemon, state_alias),
            },
        }
        upper_bin = self.world.bin.parent / self.world.bin.name.upper()
        upper_state = self.world.state.parent / self.world.state.name.upper()
        if upper_bin.exists() and upper_state.exists():
            cases["case-variant daemon only"] = {
                **base,
                "ProgramArguments": arguments(upper_bin / "kontor-daemon", other_state),
            }
            cases["case-variant state root only"] = {
                **base,
                "ProgramArguments": arguments(other_daemon, upper_state),
            }
        for case, document in cases.items():
            with self.subTest(case=case):
                plist = self.world.agents / f"{DISPOSABLE_LABEL}-{case.replace(' ', '-')}.plist"
                plist.write_bytes(plistlib.dumps(document))
                recorder = Recorder()
                argv = [
                    "--label", DISPOSABLE_LABEL,
                    "--domain", DISPOSABLE_DOMAIN,
                    "--plist", str(plist),
                    "bootstrap",
                ]
                code = run_main(argv, self.world.host(recorder, DISPOSABLE_UID))
                self.assertEqual(code, deploy.EXIT_REFUSED)
                self.assertEqual(recorder.launchctl(), [])

    def test_a_recorded_production_target_is_refused_when_the_canonical_plist_is_absent(
        self,
    ) -> None:
        production_bin = self.world.home / "prod-bin"
        production_state = self.world.home / "prod-state"
        production_bin.mkdir()
        production_state.mkdir()
        for name in deploy.BINARIES:
            (production_bin / name).write_bytes(f"prod {name}\n".encode())
        canonical = self.world.plist(
            PRODUCTION_LABEL,
            daemon=production_bin / "kontor-daemon",
            state_root=production_state,
        )
        recorder = Recorder()
        declared = [
            "--label", PRODUCTION_LABEL,
            "--domain", REAL_GUI_DOMAIN,
            "--plist", str(canonical),
            "--production-deploy", "ASMA-8506",
            "bootstrap",
        ]
        self.assertEqual(run_main(declared, self.world.host(recorder, REAL_UID)), 0)
        record = self.world.home / deploy.DEPLOY_TARGET_RELATIVE
        self.assertEqual(
            json.loads(record.read_text()),
            {
                "daemon": str(production_bin / "kontor-daemon"),
                "stateRoot": str(production_state),
                "port": 47717,
            },
        )
        canonical.unlink()
        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=production_bin / "kontor-daemon",
            state_root=production_state,
            port=47718,
        )
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(disposable),
            *command_argv("replace", self.world),
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), deploy.EXIT_REFUSED
        )
        self.assertEqual(recorder.launchctl(), [])
        self.assertFalse(self.world.backup.exists())

    def test_the_loaded_production_job_seeds_the_target_without_a_plist_or_record(
        self,
    ) -> None:
        production_bin = self.world.home / "prod-bin"
        production_state = self.world.home / "prod-state"
        production_bin.mkdir()
        production_state.mkdir()
        production_daemon = production_bin / "kontor-daemon"
        production_daemon.write_bytes(b"prod daemon\n")
        loaded = (
            f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL} = {{\n"
            f"\tprogram = {production_daemon}\n"
            "\targuments = {\n"
            f"\t\t{production_daemon}\n"
            "\t\t--state-root\n"
            f"\t\t{production_state}\n"
            "\t\t--port\n"
            "\t\t47717\n"
            "\t}\n"
            "}\n"
        )

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(0, loaded)
            return done()

        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=production_daemon,
            state_root=production_state,
        )
        recorder = Recorder(answer)
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(disposable),
            *command_argv("replace", self.world),
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)),
            deploy.EXIT_REFUSED,
        )
        self.assertEqual(
            recorder.prints(),
            [["launchctl", "print", f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL}"]],
        )
        self.assertEqual(recorder.launchctl(), [])
        self.assertFalse(self.world.backup.exists())

    def test_the_loaded_production_job_resolves_a_relative_state_root(self) -> None:
        production_bin = self.world.home / "prod-bin"
        production_bin.mkdir()
        production_daemon = production_bin / "kontor-daemon"
        production_daemon.write_bytes(b"prod daemon\n")
        other_bin = self.world.home / "other-bin"
        other_bin.mkdir()
        other_daemon = other_bin / "kontor-daemon"
        other_daemon.write_bytes(b"other daemon\n")
        workdir = self.world.home / "prod-work"
        workdir.mkdir()
        loaded = (
            f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL} = {{\n"
            f"\tprogram = {production_daemon}\n"
            "\targuments = {\n"
            f"\t\t{production_daemon}\n"
            "\t\t--state-root\n"
            "\t\tkontor/state\n"
            "\t\t--port\n"
            "\t\t47717\n"
            "\t}\n"
            f"\tworking directory = {workdir}\n"
            "}\n"
        )

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(0, loaded)
            return done()

        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=other_daemon,
            state_root="kontor/state",
            working_directory=workdir,
            port=47718,
        )
        recorder = Recorder(answer)
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(disposable),
            *command_argv("replace", self.world),
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)),
            deploy.EXIT_REFUSED,
        )
        self.assertEqual(
            recorder.prints(),
            [["launchctl", "print", f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL}"]],
        )
        self.assertEqual(recorder.launchctl(), [])
        self.assertFalse(self.world.backup.exists())

    def test_a_stale_record_does_not_hide_the_loaded_production_job(self) -> None:
        old_bin = self.world.home / "old-bin"
        old_state = self.world.home / "old-state"
        new_bin = self.world.home / "new-bin"
        new_state = self.world.home / "new-state"
        for directory in (old_bin, old_state, new_bin, new_state):
            directory.mkdir()
        for name in deploy.BINARIES:
            (old_bin / name).write_bytes(f"old {name}\n".encode())
            (new_bin / name).write_bytes(f"new {name}\n".encode())
        record = self.world.home / deploy.DEPLOY_TARGET_RELATIVE
        record.parent.mkdir(parents=True)
        record.write_text(
            json.dumps(
                {
                    "daemon": str(old_bin / "kontor-daemon"),
                    "stateRoot": str(old_state),
                    "port": 47717,
                }
            )
        )
        loaded = (
            f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL} = {{\n"
            f"\tprogram = {new_bin / 'kontor-daemon'}\n"
            "\targuments = {\n"
            f"\t\t{new_bin / 'kontor-daemon'}\n"
            "\t\t--state-root\n"
            f"\t\t{new_state}\n"
            "\t\t--port\n"
            "\t\t47718\n"
            "\t}\n"
            "}\n"
        )

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(0, loaded)
            return done()

        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=new_bin / "kontor-daemon",
            state_root=new_state,
            port=47718,
        )
        recorder = Recorder(answer)
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(disposable),
            *command_argv("replace", self.world),
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)),
            deploy.EXIT_REFUSED,
        )
        self.assertEqual(
            recorder.prints(),
            [["launchctl", "print", f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL}"]],
        )
        self.assertEqual(recorder.launchctl(), [])
        self.assertFalse(self.world.backup.exists())

    def test_a_disposable_job_cannot_take_the_production_port(self) -> None:
        self.world.plist(PRODUCTION_LABEL, port=7717)
        other_bin = self.world.home / "other-bin"
        other_state = self.world.home / "other-state"
        other_bin.mkdir()
        other_state.mkdir()
        other_daemon = other_bin / "kontor-daemon"
        other_daemon.write_bytes(b"other daemon\n")
        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=other_daemon,
            state_root=other_state,
            port=7717,
        )
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(disposable),
            "bootstrap",
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)),
            deploy.EXIT_REFUSED,
        )
        self.assertEqual(recorder.launchctl(), [])

    def test_a_disposable_job_cannot_take_the_default_production_port(self) -> None:
        self.world.plist(PRODUCTION_LABEL, port=None)
        other_bin = self.world.home / "other-bin"
        other_state = self.world.home / "other-state"
        other_bin.mkdir()
        other_state.mkdir()
        other_daemon = other_bin / "kontor-daemon"
        other_daemon.write_bytes(b"other daemon\n")
        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=other_daemon,
            state_root=other_state,
            port=7717,
        )
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(disposable),
            "bootstrap",
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)),
            deploy.EXIT_REFUSED,
        )
        self.assertEqual(recorder.launchctl(), [])

    def test_a_broken_canonical_plist_does_not_block_a_disposable_job(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL)
        canonical.write_bytes(b"not a plist")
        plist = self.world.plist(DISPOSABLE_LABEL)
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(plist),
            "bootstrap",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(len(recorder.launchctl()), 1)

    def test_a_broken_canonical_plist_still_compares_the_loaded_target(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL)
        canonical.write_bytes(b"not a plist")
        production_bin = self.world.home / "prod-bin"
        production_state = self.world.home / "prod-state"
        production_bin.mkdir()
        production_state.mkdir()
        production_daemon = production_bin / "kontor-daemon"
        production_daemon.write_bytes(b"prod daemon\n")
        loaded = (
            f"gui/{DISPOSABLE_UID}/{PRODUCTION_LABEL} = {{\n"
            f"\tprogram = {production_daemon}\n"
            "\targuments = {\n"
            f"\t\t{production_daemon}\n"
            "\t\t--state-root\n"
            f"\t\t{production_state}\n"
            "\t\t--port\n"
            "\t\t47717\n"
            "\t}\n"
            "}\n"
        )

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(0, loaded)
            return done()

        disposable = self.world.plist(
            DISPOSABLE_LABEL,
            daemon=production_daemon,
            state_root=production_state,
        )
        recorder = Recorder(answer)
        with self.assertRaises(deploy.Refused) as refusal:
            deploy.resolve_job(
                DISPOSABLE_LABEL,
                DISPOSABLE_DOMAIN,
                disposable,
                None,
                self.world.host(recorder, DISPOSABLE_UID),
                "bootstrap",
            )
        self.assertEqual(refusal.exception.code, "production_target")
        self.assertEqual(len(recorder.prints()), 1)

    def test_a_production_deploy_accepts_the_default_port(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL, port=None)
        recorder = Recorder()
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(canonical),
            "--production-deploy", "ASMA-8506",
            "bootstrap",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(len(recorder.launchctl()), 1)
        job = deploy.resolve_job(
            PRODUCTION_LABEL,
            DISPOSABLE_DOMAIN,
            canonical,
            "ASMA-8506",
            self.world.host(Recorder(), DISPOSABLE_UID),
            "bootstrap",
        )
        self.assertEqual(job.port, deploy.DEFAULT_PORT)

    def test_the_guard_reads_the_domain_users_home(self) -> None:
        target_home = self.world.home / "target-home"
        (target_home / "Library" / "LaunchAgents").mkdir(parents=True)
        canonical = (
            target_home
            / "Library"
            / "LaunchAgents"
            / f"{PRODUCTION_LABEL}.plist"
        )
        canonical.write_bytes(
            plistlib.dumps(
                {
                    "Label": PRODUCTION_LABEL,
                    "ProgramArguments": [
                        str(self.world.bin / "kontor-daemon"),
                        "--state-root",
                        str(self.world.state),
                        "--port",
                        "47717",
                    ],
                }
            )
        )
        root_home = self.world.home / "root-home"
        root_home.mkdir()
        recorder = Recorder()
        host = deploy.Host(
            uid=0,
            home=root_home,
            run=recorder.run,
            sleep=lambda seconds: None,
            homes={REAL_UID: target_home},
        )
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", f"gui/{REAL_UID}",
            "--plist", str(self.world.plist(DISPOSABLE_LABEL)),
            "bootstrap",
        ]
        self.assertEqual(run_main(argv, host), deploy.EXIT_REFUSED)
        self.assertEqual(recorder.launchctl(), [])

    def test_a_declared_production_deploy_uses_the_domain_users_home(self) -> None:
        target_home = self.world.home / "target-home"
        (target_home / "Library" / "LaunchAgents").mkdir(parents=True)
        canonical = (
            target_home
            / "Library"
            / "LaunchAgents"
            / f"{PRODUCTION_LABEL}.plist"
        )
        canonical.write_bytes(
            plistlib.dumps(
                {
                    "Label": PRODUCTION_LABEL,
                    "ProgramArguments": [
                        str(self.world.bin / "kontor-daemon"),
                        "--state-root",
                        str(self.world.state),
                        "--port",
                        "47717",
                    ],
                }
            )
        )
        root_home = self.world.home / "root-home"
        root_home.mkdir()
        recorder = Recorder()
        host = deploy.Host(
            uid=0,
            home=root_home,
            run=recorder.run,
            sleep=lambda seconds: None,
            homes={REAL_UID: target_home},
        )
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", f"gui/{REAL_UID}",
            "--plist", str(canonical),
            "--production-deploy", "ASMA-8506",
            "bootstrap",
        ]
        self.assertEqual(run_main(argv, host), 0)
        self.assertTrue((target_home / deploy.DEPLOY_TARGET_RELATIVE).is_file())
        self.assertFalse((root_home / deploy.DEPLOY_TARGET_RELATIVE).exists())

    def test_an_unparseable_loaded_production_job_is_refused(self) -> None:
        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(0, "garbage\n")
            return done()

        recorder = Recorder(answer)
        with self.assertRaises(deploy.Refused) as refusal:
            deploy.resolve_job(
                DISPOSABLE_LABEL,
                DISPOSABLE_DOMAIN,
                self.world.plist(DISPOSABLE_LABEL),
                None,
                self.world.host(recorder, DISPOSABLE_UID),
                "bootstrap",
            )
        self.assertEqual(refusal.exception.code, "production_target_unreadable")

    def test_a_failed_loaded_production_print_is_refused(self) -> None:
        failures = {
            "timeout": subprocess.TimeoutExpired(["launchctl"], deploy.DEFAULT_TIMEOUT),
            "missing launchctl": OSError("launchctl is missing"),
        }
        for case, failure in failures.items():
            with self.subTest(failure=case):
                def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
                    if argv[:2] == ["launchctl", "print"]:
                        raise failure
                    return done()

                recorder = Recorder(answer)
                with self.assertRaises(deploy.Refused) as refusal:
                    deploy.resolve_job(
                        DISPOSABLE_LABEL,
                        DISPOSABLE_DOMAIN,
                        self.world.plist(DISPOSABLE_LABEL),
                        None,
                        self.world.host(recorder, DISPOSABLE_UID),
                        "bootstrap",
                    )
                self.assertEqual(
                    refusal.exception.code, "production_target_unreadable"
                )
                self.assertEqual(recorder.launchctl(), [])

    def test_a_production_deploy_cannot_be_declared_for_another_label(self) -> None:
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(DISPOSABLE_LABEL)),
            "--production-deploy", "ASMA-8506",
            "bootout",
        ]
        self.assertEqual(
            run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), deploy.EXIT_REFUSED
        )
        self.assertEqual(recorder.calls, [])

    def test_a_declared_production_deploy_reaches_launchctl_through_the_canonical_plist(
        self,
    ) -> None:
        # A recorded host with a disposable uid: the real GUI domain never appears.
        recorder = Recorder()
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(PRODUCTION_LABEL)),
            "--production-deploy", "ASMA-8506",
            "bootout",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(
            recorder.launchctl(),
            [["launchctl", "bootout", f"{DISPOSABLE_DOMAIN}/{PRODUCTION_LABEL}"]],
        )


class ExplicitTargetTest(WorldTest):
    def test_label_and_domain_have_no_default(self) -> None:
        plist = str(self.world.plist(DISPOSABLE_LABEL))
        given = {"--label": DISPOSABLE_LABEL, "--domain": DISPOSABLE_DOMAIN, "--plist": plist}
        for missing in ("--label", "--domain"):
            with self.subTest(missing=missing):
                recorder = Recorder()
                argv = [
                    part
                    for flag, value in given.items()
                    if flag != missing
                    for part in (flag, value)
                ]
                with self.assertRaises(SystemExit) as usage:
                    run_main([*argv, "bootout"], self.world.host(recorder, DISPOSABLE_UID))
                self.assertEqual(usage.exception.code, 2)
                self.assertEqual(recorder.calls, [])

    def test_bootstrap_and_replace_still_need_a_plist(self) -> None:
        target = ["--label", DISPOSABLE_LABEL, "--domain", DISPOSABLE_DOMAIN]
        for command in ("bootstrap", "replace"):
            with self.subTest(command=command):
                recorder = Recorder()
                argv = [*target, *command_argv(command, self.world)]
                with self.assertRaises(SystemExit) as usage:
                    run_main(argv, self.world.host(recorder, DISPOSABLE_UID))
                self.assertEqual(usage.exception.code, 2)
                self.assertEqual(recorder.calls, [])

    def test_bootout_needs_no_plist(self) -> None:
        recorder = Recorder()
        argv = ["--label", DISPOSABLE_LABEL, "--domain", DISPOSABLE_DOMAIN, "bootout"]
        self.assertEqual(run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(
            recorder.launchctl(),
            [["launchctl", "bootout", f"{DISPOSABLE_DOMAIN}/{DISPOSABLE_LABEL}"]],
        )
        self.assertEqual([call for call in recorder.calls if call[0] == "lsof"], [])

    def test_bootout_stops_the_job_when_the_plist_is_corrupt(self) -> None:
        plist = self.world.agents / f"{DISPOSABLE_LABEL}.plist"
        plist.write_bytes(b"not a plist")
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(plist),
            "bootout",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(
            recorder.launchctl(),
            [["launchctl", "bootout", f"{DISPOSABLE_DOMAIN}/{DISPOSABLE_LABEL}"]],
        )
        self.assertEqual([call for call in recorder.calls if call[0] == "lsof"], [])

    def test_a_declared_production_bootout_survives_a_broken_plist(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL)
        canonical.write_bytes(b"not a plist")
        recorder = Recorder()
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", REAL_GUI_DOMAIN,
            "--plist", str(canonical),
            "--production-deploy", "ASMA-8506",
            "bootout",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, REAL_UID)), 0)
        self.assertEqual(
            recorder.launchctl(),
            [["launchctl", "bootout", f"{REAL_GUI_DOMAIN}/{PRODUCTION_LABEL}"]],
        )
        self.assertEqual([call for call in recorder.calls if call[0] == "lsof"], [])

    def test_bootout_ignores_an_unreadable_production_plist(self) -> None:
        canonical = self.world.plist(PRODUCTION_LABEL)
        canonical.write_bytes(b"not a plist")
        plist = self.world.plist(DISPOSABLE_LABEL)
        recorder = Recorder()
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(plist),
            "bootout",
        ]
        self.assertEqual(run_main(argv, self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(
            recorder.launchctl(),
            [["launchctl", "bootout", f"{DISPOSABLE_DOMAIN}/{DISPOSABLE_LABEL}"]],
        )

    def test_a_relative_state_root_resolves_against_the_working_directory(self) -> None:
        workdir = self.world.home / "launchd-workdir"
        workdir.mkdir()
        plist = self.world.plist(
            DISPOSABLE_LABEL, state_root="kontor/state", working_directory=workdir
        )
        recorder = Recorder()
        job = deploy.resolve_job(
            DISPOSABLE_LABEL,
            DISPOSABLE_DOMAIN,
            plist,
            None,
            self.world.host(recorder, DISPOSABLE_UID),
            "bootstrap",
        )
        self.assertEqual(job.state_root, (workdir / "kontor" / "state").resolve())
        self.assertTrue(job.state_root.is_absolute())

    def test_a_relative_state_root_without_a_working_directory_uses_root(self) -> None:
        plist = self.world.plist(DISPOSABLE_LABEL, state_root="kontor/state")
        recorder = Recorder()
        job = deploy.resolve_job(
            DISPOSABLE_LABEL,
            DISPOSABLE_DOMAIN,
            plist,
            None,
            self.world.host(recorder, DISPOSABLE_UID),
            "bootstrap",
        )
        self.assertEqual(job.state_root, Path("/kontor/state"))
        self.assertTrue(job.state_root.is_absolute())

    def test_commands_address_only_the_named_job(self) -> None:
        plist = self.world.plist(DISPOSABLE_LABEL)
        target = ["--label", DISPOSABLE_LABEL, "--domain", DISPOSABLE_DOMAIN, "--plist", str(plist)]
        recorder = Recorder(lambda argv: done(1) if argv[0] == "lsof" else done())
        self.assertEqual(run_main([*target, "bootout"], self.world.host(recorder, DISPOSABLE_UID)), 0)
        self.assertEqual(
            recorder.launchctl(),
            [["launchctl", "bootout", f"{DISPOSABLE_DOMAIN}/{DISPOSABLE_LABEL}"]],
        )

        submitted: list[tuple[Path, bytes]] = []

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(1)
            if argv[0] == "launchctl" and argv[1] == "bootstrap":
                submitted.append((Path(argv[3]), Path(argv[3]).read_bytes()))
            return done(1) if argv[0] == "lsof" else done()

        recorder = Recorder(answer)
        self.assertEqual(
            run_main([*target, "bootstrap"], self.world.host(recorder, DISPOSABLE_UID)), 0
        )
        calls = recorder.launchctl()
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][:3], ["launchctl", "bootstrap", DISPOSABLE_DOMAIN])
        [(submitted_path, submitted_bytes)] = submitted
        self.assertEqual(plistlib.loads(submitted_bytes)["Label"], DISPOSABLE_LABEL)
        self.assertNotEqual(submitted_path, plist)

    def test_bootout_fails_while_the_port_still_listens(self) -> None:
        plist = self.world.plist(DISPOSABLE_LABEL)
        target = ["--label", DISPOSABLE_LABEL, "--domain", DISPOSABLE_DOMAIN, "--plist", str(plist)]
        recorder = Recorder(lambda argv: done(0, "4242\n") if argv[0] == "lsof" else done())
        code = run_main([*target, "bootout"], self.world.host(recorder, DISPOSABLE_UID))
        self.assertEqual(code, deploy.EXIT_FAILED)
        self.assertIn(["lsof", "-nP", "-iTCP:47717", "-sTCP:LISTEN", "-t"], recorder.calls)

    def test_the_timeout_and_wait_are_explicit_options(self) -> None:
        arguments = deploy.parse_arguments(
            [
                "--label", DISPOSABLE_LABEL,
                "--domain", DISPOSABLE_DOMAIN,
                "--timeout", "300",
                "--wait", "60",
                "bootout",
            ]
        )
        self.assertEqual(arguments.timeout, 300.0)
        self.assertEqual(arguments.wait, 60.0)

    def test_the_port_wait_is_configurable(self) -> None:
        plist = self.world.plist(DISPOSABLE_LABEL)
        recorder = Recorder(lambda argv: done(0, "4242\n") if argv[0] == "lsof" else done())
        host = self.world.host(recorder, DISPOSABLE_UID, wait=1.0)
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(plist),
            "--wait", "1",
            "bootout",
        ]
        self.assertEqual(run_main(argv, host), deploy.EXIT_FAILED)
        self.assertEqual(len([call for call in recorder.calls if call[0] == "lsof"]), 2)

    def test_real_host_passes_the_timeout(self) -> None:
        calls: list[float] = []
        original = deploy._run

        def fake_run(argv, timeout):
            calls.append(timeout)
            return done()

        deploy._run = fake_run
        self.addCleanup(setattr, deploy, "_run", original)
        host = _real_host(300, 60)
        host.run(["true"])
        self.assertEqual(calls, [300])
        self.assertEqual(host.wait, 60)


class ReplaceTest(WorldTest):
    def answer(self, readback_status: int = 200, readback_realm: str = REALM_ID):
        self.snapshot_daemon: bytes | None = None
        self.bootstrap_plist: tuple[Path, bytes] | None = None

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(1)
            if argv[0] == "launchctl":
                if argv[1] == "bootstrap":
                    self.bootstrap_plist = (Path(argv[3]), Path(argv[3]).read_bytes())
                return done()
            if argv[0] == "lsof":
                return done(1)
            if argv[0] == str(self.world.bin / "kontor-daemon") and "snapshot" in argv:
                self.snapshot_daemon = Path(argv[0]).read_bytes()
                snapshot = Path(argv[argv.index("--into") + 1]) / "kontor-realm.db"
                snapshot.write_bytes(b"snapshot\n")
                return done(
                    0,
                    f"{snapshot}\n"
                    "2026-10-19T00:00:00.000000Z  INFO kontor_daemon: "
                    "snapshot complete kept=1 pruned=0\n",
                )
            if argv[:2] == [str(self.world.bin / "kontor"), "realm-get"]:
                body = {"status": readback_status, "body": {"realm_id": readback_realm}}
                return done(0, json.dumps(body))
            raise AssertionError(f"unexpected process {argv}")

        return answer

    def replace(self, recorder: Recorder) -> int:
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(DISPOSABLE_LABEL)),
            *command_argv("replace", self.world),
        ]
        return run_main(argv, self.world.host(recorder, DISPOSABLE_UID))

    def test_replace_installs_the_candidates_and_reads_the_realm_back(self) -> None:
        recorder = Recorder(self.answer())
        self.assertEqual(self.replace(recorder), 0)
        service = f"{DISPOSABLE_DOMAIN}/{DISPOSABLE_LABEL}"
        original = self.world.agents / f"{DISPOSABLE_LABEL}.plist"
        bootout, bootstrap = recorder.launchctl()
        self.assertEqual(bootout, ["launchctl", "bootout", service])
        self.assertEqual(bootstrap[:3], ["launchctl", "bootstrap", DISPOSABLE_DOMAIN])
        self.assertNotEqual(Path(bootstrap[3]), original)
        assert self.bootstrap_plist is not None
        self.assertEqual(plistlib.loads(self.bootstrap_plist[1])["Label"], DISPOSABLE_LABEL)
        self.assertEqual(self.snapshot_daemon, b"old kontor-daemon\n")
        for name in deploy.BINARIES:
            self.assertEqual((self.world.bin / name).read_bytes(), f"new {name}\n".encode())
        old_generation = str(self.world.bin / "kontor-mcp.oldgen")
        self.assertNotEqual(os.readlink(self.world.bin / "kontor-mcp"), old_generation)
        receipt = json.loads((self.world.backup / "deployment.json").read_text())
        self.assertEqual(receipt["label"], DISPOSABLE_LABEL)
        self.assertEqual(receipt["domain"], DISPOSABLE_DOMAIN)
        self.assertIsNone(receipt["productionDeploy"])
        self.assertEqual(receipt["readback"]["body"]["realm_id"], REALM_ID)
        old_link = (self.world.backup / "kontor-mcp.old-link").read_text().strip()
        self.assertEqual(old_link, old_generation)

    def test_bootstrap_ignores_a_plist_rewritten_after_the_guard(self) -> None:
        original = self.world.agents / f"{DISPOSABLE_LABEL}.plist"
        plain = self.answer()
        submitted: list[bytes] = []
        booted = False

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            nonlocal booted
            if argv[0] == "launchctl" and argv[1] == "bootout" and not booted:
                booted = True
                result = plain(argv)
                document = plistlib.loads(original.read_bytes())
                document["Label"] = PRODUCTION_LABEL
                original.write_bytes(plistlib.dumps(document))
                return result
            if argv[0] == "launchctl" and argv[1] == "bootstrap":
                submitted.append(Path(argv[3]).read_bytes())
            return plain(argv)

        recorder = Recorder(answer)
        self.assertEqual(self.replace(recorder), 0)
        self.assertEqual(plistlib.loads(original.read_bytes())["Label"], PRODUCTION_LABEL)
        self.assertEqual([plistlib.loads(blob)["Label"] for blob in submitted], [DISPOSABLE_LABEL])
        self.assertNotIn(
            PRODUCTION_LABEL,
            " ".join(part for call in recorder.launchctl() for part in call),
        )

    def test_replace_uses_the_state_root_launchd_resolves(self) -> None:
        workdir = self.world.home / "launchd-workdir"
        workdir.mkdir()
        plist = self.world.plist(
            DISPOSABLE_LABEL, state_root="relative/state", working_directory=workdir
        )
        expected = (workdir / "relative" / "state").resolve()
        roots: list[str] = []

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(1)
            if argv[0] == "launchctl":
                return done()
            if argv[0] == "lsof":
                return done(1)
            if argv[0] == str(self.world.bin / "kontor-daemon") and "snapshot" in argv:
                roots.append(argv[argv.index("--state-root") + 1])
                snapshot = Path(argv[argv.index("--into") + 1]) / "kontor-realm.db"
                snapshot.write_bytes(b"snapshot\n")
                return done(
                    0,
                    f"{snapshot}\n"
                    "2026-10-19T00:00:00.000000Z  INFO kontor_daemon: "
                    "snapshot complete kept=1 pruned=0\n",
                )
            if argv[:2] == [str(self.world.bin / "kontor"), "realm-get"]:
                roots.append(argv[argv.index("--state-root") + 1])
                body = {"status": 200, "body": {"realm_id": REALM_ID}}
                return done(0, json.dumps(body))
            raise AssertionError(f"unexpected process {argv}")

        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(plist),
            *command_argv("replace", self.world),
        ]
        self.assertEqual(
            run_main(argv, self.world.host(Recorder(answer), DISPOSABLE_UID)), 0
        )
        self.assertEqual(roots, [str(expected), str(expected)])

    def test_replace_restores_the_old_job_when_the_realm_does_not_read_back(self) -> None:
        before = self.world.installed()
        readbacks = {
            "unavailable": {"readback_status": 503},
            "another realm": {"readback_realm": "01a00000-0000-7000-8000-000000000002"},
        }
        for case, readback in readbacks.items():
            with self.subTest(readback=case):
                recorder = Recorder(self.answer(**readback))
                self.assertEqual(self.replace(recorder), deploy.EXIT_FAILED)
                self.assertEqual(
                    [call[1] for call in recorder.launchctl()],
                    ["bootout", "bootstrap", "bootout", "bootstrap"],
                )
                installed = self.world.installed()
                for name in deploy.BINARIES:
                    self.assertEqual(installed[name], before[name])
                rollback = json.loads((self.world.backup / "rollback.json").read_text())
                self.assertTrue(rollback["newJobStopped"])
                self.assertTrue(rollback["oldBinariesRestored"])
                self.assertTrue(rollback["oldJobRestarted"])
                self.assertFalse(rollback["databaseRestored"])
                self.assertFalse((self.world.backup / "deployment.json").exists())
                shutil.rmtree(self.world.backup)

    def test_rollback_continues_when_the_new_job_cannot_be_stopped(self) -> None:
        before = self.world.installed()
        plain = self.answer(readback_status=503)
        bootouts = 0

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            nonlocal bootouts
            if argv[0] == "launchctl" and argv[1] == "bootout":
                bootouts += 1
                if bootouts == 2:
                    return subprocess.CompletedProcess(
                        args=[], returncode=5, stdout="", stderr="Boot-out failed"
                    )
            return plain(argv)

        recorder = Recorder(answer)
        self.assertEqual(self.replace(recorder), deploy.EXIT_FAILED)
        self.assertEqual(
            [call[1] for call in recorder.launchctl()],
            ["bootout", "bootstrap", "bootout", "bootstrap"],
        )
        installed = self.world.installed()
        for name in deploy.BINARIES:
            self.assertEqual(installed[name], before[name])
        rollback = json.loads((self.world.backup / "rollback.json").read_text())
        self.assertFalse(rollback["newJobStopped"])
        self.assertTrue(rollback["oldBinariesRestored"])
        self.assertTrue(rollback["oldJobRestarted"])
        self.assertIn("newJobStopped", rollback["rollbackErrors"])

    def test_replace_restarts_the_old_job_when_the_port_never_frees(self) -> None:
        before = self.world.installed()

        def answer(argv: list[str]) -> subprocess.CompletedProcess[str]:
            if argv[:2] == ["launchctl", "print"]:
                return done(1)
            if argv[0] == "launchctl":
                return done()
            if argv[0] == "lsof":
                return done(0, "4242\n")
            raise AssertionError(f"unexpected process {argv}")

        recorder = Recorder(answer)
        self.assertEqual(self.replace(recorder), deploy.EXIT_FAILED)
        self.assertEqual([call[1] for call in recorder.launchctl()], ["bootout", "bootstrap"])
        self.assertEqual(self.world.installed(), before)
        rollback = json.loads((self.world.backup / "rollback.json").read_text())
        self.assertFalse(rollback["newJobStopped"])
        self.assertFalse(rollback["oldBinariesRestored"])
        self.assertTrue(rollback["oldJobRestarted"])

    def test_replace_never_applies_twice(self) -> None:
        self.world.backup.mkdir()
        recorder = Recorder(self.answer())
        self.assertEqual(self.replace(recorder), deploy.EXIT_FAILED)
        self.assertEqual(recorder.launchctl(), [])

    def production_replace(
        self, recorder: Recorder, expected: list[str]
    ) -> int:
        argv = [
            "--label", PRODUCTION_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(PRODUCTION_LABEL)),
            "--production-deploy", "ASMA-8506",
            *command_argv("replace", self.world),
            *[
                part
                for value in expected
                for part in ("--expected-sha256", value)
            ],
        ]
        return run_main(argv, self.world.host(recorder, DISPOSABLE_UID))

    def test_a_declared_production_replace_needs_expected_hashes(self) -> None:
        recorder = Recorder(self.answer())
        self.assertEqual(self.production_replace(recorder, []), deploy.EXIT_FAILED)
        self.assertEqual(recorder.launchctl(), [])
        self.assertFalse(self.world.backup.exists())

    def test_a_declared_production_replace_refuses_unexpected_candidate_bytes(self) -> None:
        recorder = Recorder(self.answer())
        wrong = [f"{name}={'0' * 64}" for name in deploy.BINARIES]
        self.assertEqual(self.production_replace(recorder, wrong), deploy.EXIT_FAILED)
        self.assertEqual(recorder.launchctl(), [])
        self.assertFalse(self.world.backup.exists())

    def test_a_declared_production_replace_pins_the_candidate_hashes(self) -> None:
        expected = {name: deploy.digest(self.world.candidates / name) for name in deploy.BINARIES}
        recorder = Recorder(self.answer())
        code = self.production_replace(
            recorder, [f"{name}={checksum}" for name, checksum in expected.items()]
        )
        self.assertEqual(code, 0)
        receipt = json.loads((self.world.backup / "deployment.json").read_text())
        self.assertEqual(receipt["productionDeploy"], "ASMA-8506")
        self.assertEqual(receipt["binarySha256After"], expected)
        for name in deploy.BINARIES:
            self.assertEqual((self.world.bin / name).read_bytes(), f"new {name}\n".encode())

    def test_the_realm_get_wait_is_configurable(self) -> None:
        recorder = Recorder(self.answer(readback_status=503))
        argv = [
            "--label", DISPOSABLE_LABEL,
            "--domain", DISPOSABLE_DOMAIN,
            "--plist", str(self.world.plist(DISPOSABLE_LABEL)),
            "--wait", "1",
            *command_argv("replace", self.world),
        ]
        code = run_main(argv, self.world.host(recorder, DISPOSABLE_UID, wait=1.0))
        self.assertEqual(code, deploy.EXIT_FAILED)
        polls = [
            call
            for call in recorder.calls
            if call[:2] == [str(self.world.bin / "kontor"), "realm-get"]
        ]
        self.assertEqual(len(polls), 2)


if __name__ == "__main__":
    unittest.main()
