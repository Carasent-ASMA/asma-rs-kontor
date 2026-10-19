#!/usr/bin/env python3
"""Deploy Kontor into one launchd job, named only by explicit arguments.

This is the one deploy script for a Kontor daemon that runs as a launchd job.
It replaces the per-ticket ``deployment-apply.py`` copies, which hard-coded the
production label and the real ``gui/<uid>`` domain. A test harness copied that
pattern, booted out the production job and bootstrapped its own plist under the
production label (gap G-01).

``--label`` and ``--domain`` are required, and ``--plist`` is required except
for ``bootout``. None has a default, an environment fallback or a value read
from a file. Every command passes the target guard (``resolve_job``) before it
changes any job:

* the production label ``com.asma.kontor.daemon`` is refused unless
  ``--production-deploy <JIRA-KEY>`` declares the deploy;
* a declared production deploy must name the ``gui/<uid>`` domain of the user
  whose home holds ``~/Library/LaunchAgents/com.asma.kontor.daemon.plist``;
* ``--production-deploy`` with any other label is refused;
* a non-production ``bootstrap`` or ``replace`` whose daemon or state root is
  the production one, or that uses the production port, is refused even under
  another label, because its install would overwrite the production binaries or
  take their port; the guard compares every source it can read: the canonical
  production plist, the target recorded by the last declared production deploy,
  and the loaded production job read with ``launchctl print``. An unreadable
  canonical plist is skipped, while a loaded job whose print cannot be parsed
  is refused;
* a relative ``--state-root`` is resolved against the plist's
  ``WorkingDirectory``, or ``/`` when it has none, so the snapshot and read-back
  address the state root launchd serves;
* the plist's ``Label`` must equal ``--label``, because ``launchctl bootstrap``
  registers the label written in the plist, not the one on the command line;
  the validated bytes are copied to a private snapshot and every command uses
  that copy, so an edit after the guard cannot change what launchctl loads.

Commands:

  bootout    stop the job, then wait until its port no longer listens; works
             without --plist and with an unreadable plist, because it needs
             only the label and domain
  bootstrap  start the job from --plist
  replace    the deploy: back up the installed binaries, stop the job,
             snapshot the realm with the installed daemon, install the
             candidates, start the job and read the realm back. A failure
             after the stop restores the old binaries and restarts the job.
             The database is not restored: the snapshot in --backup-dir
             restores it per RECOVERY.md. A declared production replace
             needs ``--expected-sha256`` for each candidate and compares
             them before the job is stopped.

The daemon binary, state root and port come from the plist's
``ProgramArguments``; a missing ``--port`` uses the daemon default 7717, and
``kontor`` and ``kontor-mcp`` are installed beside the daemon. ``--timeout``
caps each subprocess (default 120 s) and ``--wait`` bounds the port and
realm-get polls (default 15 s).

Exit codes: 0 done, 1 failed (after any rollback), 2 usage, 3 refused by the
target guard (no launchctl mutation was attempted).

Tests use a disposable label and an injected runner, and never reach launchctl:

    PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -v
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import plistlib
import pwd
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from collections.abc import Callable, Sequence
from xml.parsers.expat import ExpatError

PRODUCTION_LABEL = "com.asma.kontor.daemon"
DEPLOY_TARGET_RELATIVE = Path(".local") / "state" / "kontor" / "deploy-target.json"
DEFAULT_PORT = 7717
DEFAULT_TIMEOUT = 120.0
DEFAULT_WAIT = 15.0
BINARIES = ("kontor-daemon", "kontor", "kontor-mcp")
LABEL_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9.-]*")
DOMAIN_PATTERN = re.compile(r"(gui|user)/[0-9]+")
DEPLOY_PATTERN = re.compile(r"[A-Z][A-Z0-9]*-[1-9][0-9]*")
POLL_INTERVAL = 0.5

EXIT_FAILED = 1
EXIT_REFUSED = 3

Runner = Callable[[Sequence[str]], "subprocess.CompletedProcess[str]"]


@dataclass(frozen=True)
class Host:
    """Everything the script touches outside its arguments and files."""

    uid: int
    home: Path
    run: Runner
    sleep: Callable[[float], None]
    homes: dict[int, Path] | None = None
    wait: float = DEFAULT_WAIT

    def home_for(self, uid: int) -> Path:
        if uid == self.uid:
            return self.home
        if self.homes is not None and uid in self.homes:
            return self.homes[uid]
        try:
            return Path(pwd.getpwuid(uid).pw_dir)
        except KeyError as error:
            raise Refused("domain_uid", f"uid {uid} has no password entry") from error


def real_host(timeout: float = DEFAULT_TIMEOUT, wait: float = DEFAULT_WAIT) -> Host:
    """This user, read from the password database rather than ``$HOME``."""
    uid = os.getuid()
    return Host(
        uid=uid,
        home=Path(pwd.getpwuid(uid).pw_dir),
        run=lambda argv: _run(argv, timeout),
        sleep=time.sleep,
        wait=wait,
    )


def _run(
    argv: Sequence[str], timeout: float = DEFAULT_TIMEOUT
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(list(argv), capture_output=True, text=True, timeout=timeout)


class Refused(Exception):
    """The target guard refused the job; no launchctl mutation was attempted."""

    def __init__(self, code: str, rule: str) -> None:
        super().__init__(f"{code}: {rule}")
        self.code = code


class Failed(Exception):
    """A step failed after the guard passed."""


ROLLBACK_ERRORS = (Failed, OSError, subprocess.SubprocessError)


@dataclass(frozen=True)
class ProductionTarget:
    """Where the production job runs, from any source the guard can read."""

    daemon: Path
    state_root: Path
    port: int | None


@dataclass(frozen=True)
class Job:
    """One launchd job, as the guard accepted it."""

    label: str
    domain: str
    plist: Path | None
    daemon: Path | None
    state_root: Path | None
    port: int | None

    @property
    def service(self) -> str:
        return f"{self.domain}/{self.label}"

    @property
    def install_dir(self) -> Path:
        assert self.daemon is not None
        return self.daemon.parent


def resolve_job(
    label: str,
    domain: str,
    plist: Path | None,
    production_deploy: str | None,
    host: Host,
    command: str,
) -> Job:
    """The target guard: name the job, or refuse before any process starts."""
    if not LABEL_PATTERN.fullmatch(label):
        raise Refused("label_invalid", f"{label!r} is not a launchd label")
    if not DOMAIN_PATTERN.fullmatch(domain):
        raise Refused("domain_invalid", f"{domain!r} is not gui/<uid> or user/<uid>")
    domain_uid = int(domain.split("/", 1)[1])
    production_home = host.home_for(domain_uid)
    if label.casefold() == PRODUCTION_LABEL:
        if production_deploy is None:
            raise Refused(
                "production_undeclared",
                f"{PRODUCTION_LABEL} is the production job; only a deploy declared "
                "with --production-deploy <JIRA-KEY> may address it, never a test",
            )
        if not DEPLOY_PATTERN.fullmatch(production_deploy):
            raise Refused(
                "production_deploy_invalid",
                f"{production_deploy!r} is not the Jira key of the declared deploy",
            )
        if domain != f"gui/{domain_uid}":
            raise Refused(
                "production_domain",
                f"the production job lives in gui/{domain_uid}, not {domain}",
            )
        canonical = (
            production_home / "Library" / "LaunchAgents" / f"{PRODUCTION_LABEL}.plist"
        )
        if plist is not None and plist.resolve() != canonical.resolve():
            raise Refused(
                "production_plist",
                f"the production job is bootstrapped only from {canonical}, not {plist}",
            )
    elif production_deploy is not None:
        raise Refused(
            "production_deploy_misdirected",
            f"--production-deploy declares a production deploy, but {label} is not "
            f"{PRODUCTION_LABEL}",
        )
    if plist is None:
        if command != "bootout":
            raise Refused("plist_required", f"{command} needs --plist")
        return Job(label, domain, None, None, None, None)
    try:
        data, document = _read_plist(plist)
        daemon, state_root, port = _program(document, plist)
    except Refused:
        if command == "bootout":
            return Job(label, domain, None, None, None, None)
        raise
    if document.get("Label") != label:
        raise Refused(
            "plist_label_mismatch",
            f"{plist} registers {document.get('Label')!r}; launchctl would act on "
            f"that label, not {label!r}",
        )
    if label.casefold() == PRODUCTION_LABEL:
        if command != "bootout":
            _record_production_target(production_home, daemon, state_root, port)
    elif command != "bootout":
        canonical = (
            production_home / "Library" / "LaunchAgents" / f"{PRODUCTION_LABEL}.plist"
        )
        targets: list[ProductionTarget] = []
        if canonical.is_file():
            try:
                targets.append(_job_target(_read_plist(canonical)[1], canonical))
            except Refused:
                pass
        recorded = _recorded_production_target(production_home)
        if recorded is not None:
            targets.append(recorded)
        loaded = _loaded_production_target(host, domain_uid)
        if loaded is not None:
            targets.append(loaded)
        for target in targets:
            target_port = target.port if target.port is not None else DEFAULT_PORT
            if (
                _same_path(daemon, target.daemon)
                or _same_path(state_root, target.state_root)
                or port == target_port
            ):
                raise Refused(
                    "production_target",
                    f"{plist} runs {daemon} with state root {state_root} on port {port}, "
                    "which the production job uses; only --production-deploy may "
                    "address it",
                )
    if command == "bootout":
        return Job(label, domain, None, None, None, port)
    snapshot = _snapshot_plist(data, label)
    return Job(label, domain, snapshot, daemon, state_root, port)


def _read_plist(plist: Path) -> tuple[bytes, dict]:
    """The plist bytes and their parse, so the guard and the snapshot agree."""
    try:
        data = plist.read_bytes()
        document = plistlib.loads(data)
    except (OSError, ValueError, ExpatError) as error:
        raise Refused("plist_unreadable", f"{plist}: {error}") from error
    if not isinstance(document, dict):
        raise Refused("plist_unreadable", f"{plist} is not a launchd job dictionary")
    return data, document


def _snapshot_plist(data: bytes, label: str) -> Path:
    """A private copy of the bytes the guard read, for launchctl to load."""
    try:
        directory = Path(tempfile.mkdtemp(prefix="kontor-deploy-"))
        os.chmod(directory, 0o700)
        snapshot = directory / f"{label}.plist"
        snapshot.write_bytes(data)
        os.chmod(snapshot, 0o600)
    except OSError as error:
        raise Refused("plist_snapshot", f"cannot write a private plist snapshot: {error}") from error
    return snapshot


def _production_target_path(home: Path) -> Path:
    return home / DEPLOY_TARGET_RELATIVE


def _record_production_target(
    home: Path, daemon: Path, state_root: Path, port: int
) -> None:
    path = _production_target_path(home)
    try:
        path.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
        _write_json(
            path, {"daemon": str(daemon), "stateRoot": str(state_root), "port": port}
        )
    except OSError as error:
        raise Refused(
            "production_target_record",
            f"cannot record the production target at {path}: {error}",
        ) from error


def _recorded_production_target(home: Path) -> ProductionTarget | None:
    try:
        document = json.loads(_production_target_path(home).read_text())
    except (OSError, ValueError):
        return None
    if not isinstance(document, dict):
        return None
    daemon = document.get("daemon")
    state_root = document.get("stateRoot")
    if not isinstance(daemon, str) or not isinstance(state_root, str):
        return None
    port = document.get("port")
    if isinstance(port, int) and not isinstance(port, bool):
        port_value = port
    elif isinstance(port, str) and port.isdigit():
        port_value = int(port)
    else:
        port_value = None
    return ProductionTarget(Path(daemon), Path(state_root), port_value)


def _loaded_production_target(host: Host, domain_uid: int) -> ProductionTarget | None:
    service = f"gui/{domain_uid}/{PRODUCTION_LABEL}"
    try:
        result = host.run(["launchctl", "print", service])
    except (OSError, subprocess.SubprocessError) as error:
        raise Refused(
            "production_target_unreadable",
            f"launchctl print {service} failed: {error}",
        ) from error
    if result.returncode != 0:
        return None

    def unreadable() -> Refused:
        return Refused(
            "production_target_unreadable",
            f"launchctl print {service} did not report a readable production job",
        )

    arguments: list[str] = []
    working_directory: str | None = None
    in_arguments = False
    for raw_line in result.stdout.splitlines():
        line = raw_line.strip()
        if in_arguments:
            if line == "}":
                in_arguments = False
                continue
            if line:
                arguments.append(line.strip('"'))
            continue
        if line == "arguments = {":
            in_arguments = True
        elif line.startswith("working directory = "):
            working_directory = line.split("=", 1)[1].strip().strip('"')
    if not arguments:
        raise unreadable()
    daemon = Path(arguments[0])
    state_root = _option(arguments, "--state-root")
    if not daemon.is_absolute() or daemon.name != "kontor-daemon" or state_root is None:
        raise unreadable()
    state_root_path = Path(state_root)
    if not state_root_path.is_absolute():
        base = Path(working_directory) if working_directory else Path("/")
        if not base.is_absolute():
            raise unreadable()
        state_root_path = (base / state_root_path).resolve()
    port = _option(arguments, "--port")
    port_value = int(port) if port is not None and port.isdigit() else None
    return ProductionTarget(daemon, state_root_path, port_value)


def _same_path(left: Path, right: Path) -> bool:
    try:
        return os.path.samefile(left, right)
    except OSError:
        return left.resolve() == right.resolve()


def _job_paths(document: dict, plist: Path) -> tuple[Path, Path]:
    """The daemon and state root the job runs, from ProgramArguments."""
    arguments = document.get("ProgramArguments")
    if (
        not isinstance(arguments, list)
        or not arguments
        or not all(isinstance(argument, str) for argument in arguments)
    ):
        raise Refused("plist_program", f"{plist} has no ProgramArguments")
    daemon = Path(arguments[0])
    if not daemon.is_absolute() or daemon.name != "kontor-daemon":
        raise Refused("plist_program", f"{plist} does not run an absolute kontor-daemon")
    state_root = _option(arguments, "--state-root")
    if state_root is None:
        raise Refused("plist_program", f"{plist} must pass --state-root")
    state_root_path = Path(state_root)
    if not state_root_path.is_absolute():
        working_directory = document.get("WorkingDirectory", "/")
        if not isinstance(working_directory, str) or not working_directory:
            raise Refused("plist_program", f"{plist} has an invalid WorkingDirectory")
        base = Path(working_directory)
        if not base.is_absolute():
            raise Refused("plist_program", f"{plist} has a relative WorkingDirectory")
        state_root_path = (base / state_root_path).resolve()
    return daemon, state_root_path


def _job_target(document: dict, plist: Path) -> ProductionTarget:
    """The daemon, state root and port the job runs, from ProgramArguments."""
    daemon, state_root = _job_paths(document, plist)
    arguments = document.get("ProgramArguments")
    port = _option(arguments, "--port") if isinstance(arguments, list) else None
    return ProductionTarget(
        daemon,
        state_root,
        int(port) if port is not None and port.isdigit() else None,
    )


def _program(document: dict, plist: Path) -> tuple[Path, Path, int]:
    target = _job_target(document, plist)
    return (
        target.daemon,
        target.state_root,
        target.port if target.port is not None else DEFAULT_PORT,
    )


def _option(arguments: list[str], name: str) -> str | None:
    for index, argument in enumerate(arguments[1:], start=1):
        if argument == name and index + 1 < len(arguments):
            return arguments[index + 1]
        if argument.startswith(f"{name}="):
            return argument.split("=", 1)[1]
    return None


def launchctl(host: Host, *arguments: str) -> None:
    result = host.run(["launchctl", *arguments])
    if result.returncode != 0:
        raise Failed(
            f"launchctl {' '.join(arguments)} exited {result.returncode}: "
            f"{result.stderr.strip()}"
        )


def stop_job(job: Job, host: Host) -> None:
    """The launchctl call alone, so the caller knows the job is unloaded."""
    launchctl(host, "bootout", job.service)


def _poll_attempts(host: Host) -> int:
    return max(1, int(host.wait / POLL_INTERVAL))


def wait_port_free(job: Job, host: Host) -> None:
    if job.port is None:
        return
    for _ in range(_poll_attempts(host)):
        listening = host.run(["lsof", "-nP", f"-iTCP:{job.port}", "-sTCP:LISTEN", "-t"])
        if not listening.stdout.strip():
            return
        host.sleep(POLL_INTERVAL)
    raise Failed(f"port {job.port} still listens after the bootout of {job.service}")


def bootout(job: Job, host: Host) -> None:
    stop_job(job, host)
    wait_port_free(job, host)


def bootstrap(job: Job, host: Host) -> None:
    assert job.plist is not None
    launchctl(host, "bootstrap", job.domain, str(job.plist))


def snapshot(job: Job, host: Host, into: Path) -> Path:
    """A verified realm snapshot taken by the installed daemon (RECOVERY.md)."""
    assert job.daemon is not None and job.state_root is not None
    result = host.run(
        [str(job.daemon), "--state-root", str(job.state_root), "snapshot", "--into", str(into)]
    )
    printed = [Path(line.strip()) for line in result.stdout.splitlines()]
    path = next((candidate for candidate in reversed(printed) if candidate.is_file()), None)
    if result.returncode != 0 or path is None:
        raise Failed(
            f"the snapshot of {job.state_root} failed (exit {result.returncode}): "
            f"{result.stderr.strip()}"
        )
    return path


def read_back(job: Job, host: Host, realm_id: str) -> dict:
    """Wait until the started daemon answers realm-get for the expected realm."""
    assert job.state_root is not None
    kontor = job.install_dir / "kontor"
    for _ in range(_poll_attempts(host)):
        result = host.run([str(kontor), "realm-get", "--state-root", str(job.state_root)])
        if result.returncode == 0:
            try:
                answer = json.loads(result.stdout)
            except ValueError:
                answer = None
            if (
                isinstance(answer, dict)
                and answer.get("status") == 200
                and isinstance(answer.get("body"), dict)
                and answer["body"].get("realm_id") == realm_id
            ):
                return answer
        host.sleep(POLL_INTERVAL)
    raise Failed(f"realm {realm_id} did not read back from {job.service}")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@dataclass(frozen=True)
class Previous:
    """One binary as it was installed before the deploy."""

    name: str
    link: str | None
    backup: Path


def back_up(job: Job, backup_dir: Path) -> list[Previous]:
    previous = []
    for name in BINARIES:
        target = job.install_dir / name
        if not target.is_file():
            raise Failed(f"{target} is not installed")
        link = os.readlink(target) if target.is_symlink() else None
        copy = backup_dir / f"{name}.old"
        shutil.copy2(target, copy)
        if link is not None:
            (backup_dir / f"{name}.old-link").write_text(f"{link}\n")
        previous.append(Previous(name, link, copy))
    return previous


def install(job: Job, source: Path) -> None:
    target = job.install_dir / source.name
    if target.is_symlink():
        # A symlinked binary keeps each generation beside it: the candidate gets
        # its own name and the link swings to it in one rename.
        generation = job.install_dir / f"{source.name}.{digest(source)[:12]}"
        _place(source, generation)
        _swing(target, str(generation))
    else:
        _place(source, target)


def restore(job: Job, previous: list[Previous]) -> None:
    for old in previous:
        target = job.install_dir / old.name
        if old.link is not None:
            _swing(target, old.link)
        else:
            _place(old.backup, target)


def _place(source: Path, target: Path) -> None:
    staging = target.with_name(f"{target.name}.deploy-staging")
    shutil.copy2(source, staging)
    os.chmod(staging, 0o755)
    os.replace(staging, target)


def _swing(link: Path, target: str) -> None:
    staging = link.with_name(f"{link.name}.deploy-link")
    if staging.is_symlink() or staging.exists():
        staging.unlink()
    os.symlink(target, staging)
    os.replace(staging, link)


def _write_json(path: Path, document: dict) -> None:
    path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")


def _now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")


def replace(
    job: Job,
    host: Host,
    candidates: Path,
    backup_dir: Path,
    realm_id: str,
    production_deploy: str | None,
    expected: dict[str, str],
) -> dict:
    for name in BINARIES:
        if not (candidates / name).is_file():
            raise Failed(f"{candidates / name} is missing")
    after = {name: digest(candidates / name) for name in BINARIES}
    if production_deploy is not None:
        missing = [name for name in BINARIES if name not in expected]
        if missing:
            raise Failed(
                "a declared production replace needs --expected-sha256 for "
                + ", ".join(missing)
            )
    for name, checksum in expected.items():
        if after[name] != checksum:
            raise Failed(
                f"{candidates / name} has SHA-256 {after[name]}, not the expected "
                f"{checksum}; the job was not stopped"
            )
    try:
        backup_dir.mkdir(mode=0o700, parents=True)
    except FileExistsError:
        raise Failed(f"{backup_dir} already exists; a deploy never applies twice") from None
    previous = back_up(job, backup_dir)
    before = {old.name: digest(old.backup) for old in previous}

    stopped = installing = started = False
    try:
        stop_job(job, host)
        stopped = True
        wait_port_free(job, host)
        realm_snapshot = snapshot(job, host, backup_dir)
        installing = True
        for name in BINARIES:
            install(job, candidates / name)
            if digest(job.install_dir / name) != after[name]:
                raise Failed(f"{job.install_dir / name} does not hold the candidate")
        bootstrap(job, host)
        started = True
        readback = read_back(job, host, realm_id)
    except BaseException as error:
        rollback = {
            "at": _now(),
            "failure": str(error),
            "newJobStopped": False,
            "oldBinariesRestored": False,
            "oldJobRestarted": False,
            "rollbackErrors": {},
            "databaseRestored": False,
        }
        try:
            if started:
                try:
                    bootout(job, host)
                    rollback["newJobStopped"] = True
                except ROLLBACK_ERRORS as rollback_error:
                    rollback["rollbackErrors"]["newJobStopped"] = str(rollback_error)
            if installing:
                try:
                    restore(job, previous)
                    rollback["oldBinariesRestored"] = True
                except ROLLBACK_ERRORS as rollback_error:
                    rollback["rollbackErrors"]["oldBinariesRestored"] = str(rollback_error)
            if stopped:
                try:
                    bootstrap(job, host)
                    rollback["oldJobRestarted"] = True
                except ROLLBACK_ERRORS as rollback_error:
                    rollback["rollbackErrors"]["oldJobRestarted"] = str(rollback_error)
        finally:
            _write_json(backup_dir / "rollback.json", rollback)
        raise

    receipt = {
        "appliedAt": _now(),
        "label": job.label,
        "domain": job.domain,
        "productionDeploy": production_deploy,
        "realmId": realm_id,
        "binarySha256Before": before,
        "binarySha256After": after,
        "snapshot": realm_snapshot.name,
        "snapshotSha256": digest(realm_snapshot),
        "readback": readback,
    }
    _write_json(backup_dir / "deployment.json", receipt)
    return receipt


def parse_arguments(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="kontor-deploy.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        allow_abbrev=False,
    )
    parser.add_argument("--label", required=True, help="the job's launchd label")
    parser.add_argument(
        "--domain", required=True, help="the job's launchd domain: gui/<uid> or user/<uid>"
    )
    parser.add_argument(
        "--plist",
        type=Path,
        help="the job's plist; required unless the command is bootout",
    )
    parser.add_argument(
        "--production-deploy",
        metavar="JIRA-KEY",
        help=f"declare a production deploy of {PRODUCTION_LABEL} under this Jira key",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=DEFAULT_TIMEOUT,
        help=f"seconds to cap each subprocess (default {DEFAULT_TIMEOUT:g})",
    )
    parser.add_argument(
        "--wait",
        type=float,
        default=DEFAULT_WAIT,
        help=(
            "seconds to wait for the port to free and for realm-get to answer "
            f"(default {DEFAULT_WAIT:g})"
        ),
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("bootout", help="stop the job")
    commands.add_parser("bootstrap", help="start the job")
    deploy = commands.add_parser("replace", help="deploy new binaries into the job")
    deploy.add_argument(
        "--candidates",
        required=True,
        type=Path,
        help="directory holding the new kontor-daemon, kontor and kontor-mcp",
    )
    deploy.add_argument(
        "--backup-dir",
        required=True,
        type=Path,
        help="new directory for the old binaries, the snapshot and the receipts",
    )
    deploy.add_argument(
        "--realm-id", required=True, help="the realm the restarted daemon must read back"
    )
    deploy.add_argument(
        "--expected-sha256",
        action="append",
        default=[],
        metavar="NAME=SHA256",
        help=(
            "expected SHA-256 of a candidate binary, repeatable; a declared "
            "production replace needs one for each binary"
        ),
    )
    arguments = parser.parse_args(argv)
    if arguments.plist is None and arguments.command != "bootout":
        parser.error("--plist is required unless the command is bootout")
    if arguments.timeout <= 0:
        parser.error("--timeout must be positive")
    if arguments.wait < 0:
        parser.error("--wait must not be negative")
    expected: dict[str, str] = {}
    for value in getattr(arguments, "expected_sha256", []):
        name, separator, checksum = value.partition("=")
        if (
            not separator
            or name not in BINARIES
            or re.fullmatch(r"[0-9a-f]{64}", checksum) is None
        ):
            parser.error(
                "--expected-sha256 needs NAME=SHA256 for one of "
                + ", ".join(BINARIES)
            )
        if name in expected:
            parser.error(f"--expected-sha256 names {name} more than once")
        expected[name] = checksum
    arguments.expected_sha256 = expected
    return arguments


def main(argv: Sequence[str] | None = None, host: Host | None = None) -> int:
    arguments = parse_arguments(argv)
    host = host or real_host(arguments.timeout, arguments.wait)
    try:
        job = resolve_job(
            arguments.label,
            arguments.domain,
            arguments.plist,
            arguments.production_deploy,
            host,
            arguments.command,
        )
    except Refused as refusal:
        print(f"kontor-deploy: refused {refusal}", file=sys.stderr)
        return EXIT_REFUSED
    try:
        if arguments.command == "bootout":
            bootout(job, host)
        elif arguments.command == "bootstrap":
            bootstrap(job, host)
        else:
            receipt = replace(
                job,
                host,
                arguments.candidates,
                arguments.backup_dir,
                arguments.realm_id,
                arguments.production_deploy,
                arguments.expected_sha256,
            )
            print(json.dumps(receipt, indent=2, sort_keys=True))
    except (Failed, OSError, subprocess.SubprocessError) as error:
        print(f"kontor-deploy: failed: {error}", file=sys.stderr)
        return EXIT_FAILED
    finally:
        if job.plist is not None:
            shutil.rmtree(job.plist.parent, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
