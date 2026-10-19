#!/usr/bin/env python3
"""Deploy Kontor into one launchd job, named only by explicit arguments.

This is the one deploy script for a Kontor daemon that runs as a launchd job.
It replaces the per-ticket ``deployment-apply.py`` copies, which hard-coded the
production label and the real ``gui/<uid>`` domain. A test harness copied that
pattern, booted out the production job and bootstrapped its own plist under the
production label (gap G-01).

``--label``, ``--domain`` and ``--plist`` are required. None has a default, an
environment fallback or a value read from a file. Every command passes the
target guard (``resolve_job``) before it starts any process:

* the production label ``com.asma.kontor.daemon`` is refused unless
  ``--production-deploy <JIRA-KEY>`` declares the deploy;
* a declared production deploy must name this user's ``gui/<uid>`` domain and
  this user's ``~/Library/LaunchAgents/com.asma.kontor.daemon.plist``;
* ``--production-deploy`` with any other label is refused;
* a job whose daemon or state root is the production one is refused even under
  another label, because its install would overwrite the production binaries;
* the plist's ``Label`` must equal ``--label``, because ``launchctl bootstrap``
  registers the label written in the plist, not the one on the command line.

Commands:

  bootout    stop the job, then wait until its port no longer listens
  bootstrap  start the job from --plist
  replace    the deploy: back up the installed binaries, stop the job,
             snapshot the realm with the installed daemon, install the
             candidates, start the job and read the realm back. A failure
             after the stop restores the old binaries and restarts the job.
             The database is not restored: the snapshot in --backup-dir
             restores it per RECOVERY.md.

The daemon binary, state root and port come from the plist's
``ProgramArguments``; ``kontor`` and ``kontor-mcp`` are installed beside the
daemon.

Exit codes: 0 done, 1 failed (after any rollback), 2 usage, 3 refused by the
target guard (no process was started).

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
import time
from dataclasses import dataclass
from pathlib import Path
from collections.abc import Callable, Sequence
from xml.parsers.expat import ExpatError

PRODUCTION_LABEL = "com.asma.kontor.daemon"
BINARIES = ("kontor-daemon", "kontor", "kontor-mcp")
LABEL_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9.-]*")
DOMAIN_PATTERN = re.compile(r"(gui|user)/[0-9]+")
DEPLOY_PATTERN = re.compile(r"[A-Z][A-Z0-9]*-[1-9][0-9]*")
POLL_ATTEMPTS = 30
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


def real_host() -> Host:
    """This user, read from the password database rather than ``$HOME``."""
    uid = os.getuid()
    return Host(uid=uid, home=Path(pwd.getpwuid(uid).pw_dir), run=_run, sleep=time.sleep)


def _run(argv: Sequence[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(list(argv), capture_output=True, text=True, timeout=120)


class Refused(Exception):
    """The target guard refused the job; no process was started."""

    def __init__(self, code: str, rule: str) -> None:
        super().__init__(f"{code}: {rule}")
        self.code = code


class Failed(Exception):
    """A step failed after the guard passed."""


@dataclass(frozen=True)
class Job:
    """One launchd job, as the guard accepted it."""

    label: str
    domain: str
    plist: Path
    daemon: Path
    state_root: Path
    port: int

    @property
    def service(self) -> str:
        return f"{self.domain}/{self.label}"

    @property
    def install_dir(self) -> Path:
        return self.daemon.parent


def resolve_job(
    label: str, domain: str, plist: Path, production_deploy: str | None, host: Host
) -> Job:
    """The target guard: name the job, or refuse before any process starts."""
    if not LABEL_PATTERN.fullmatch(label):
        raise Refused("label_invalid", f"{label!r} is not a launchd label")
    if not DOMAIN_PATTERN.fullmatch(domain):
        raise Refused("domain_invalid", f"{domain!r} is not gui/<uid> or user/<uid>")
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
        if domain != f"gui/{host.uid}":
            raise Refused(
                "production_domain",
                f"the production job lives in gui/{host.uid}, not {domain}",
            )
        canonical = host.home / "Library" / "LaunchAgents" / f"{PRODUCTION_LABEL}.plist"
        if plist.resolve() != canonical.resolve():
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
    document = _read_plist(plist)
    if document.get("Label") != label:
        raise Refused(
            "plist_label_mismatch",
            f"{plist} registers {document.get('Label')!r}; launchctl would act on "
            f"that label, not {label!r}",
        )
    daemon, state_root, port = _program(document, plist)
    if label.casefold() != PRODUCTION_LABEL:
        canonical = host.home / "Library" / "LaunchAgents" / f"{PRODUCTION_LABEL}.plist"
        if canonical.is_file():
            production_daemon, production_state_root, _ = _program(
                _read_plist(canonical), canonical
            )
            if (
                daemon.resolve() == production_daemon.resolve()
                or state_root.resolve() == production_state_root.resolve()
            ):
                raise Refused(
                    "production_target",
                    f"{plist} runs {daemon} with state root {state_root}, which is the "
                    "production install; only --production-deploy may address it",
                )
    return Job(label, domain, plist, daemon, state_root, port)


def _read_plist(plist: Path) -> dict:
    try:
        document = plistlib.loads(plist.read_bytes())
    except (OSError, ValueError, ExpatError) as error:
        raise Refused("plist_unreadable", f"{plist}: {error}") from error
    if not isinstance(document, dict):
        raise Refused("plist_unreadable", f"{plist} is not a launchd job dictionary")
    return document


def _program(document: dict, plist: Path) -> tuple[Path, Path, int]:
    """The daemon, state root and port the job runs, from ProgramArguments."""
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
    port = _option(arguments, "--port")
    if state_root is None or port is None or not port.isdigit():
        raise Refused(
            "plist_program",
            f"{plist} must pass --state-root and --port: the deploy snapshots the "
            "one and waits on the other",
        )
    return daemon, Path(state_root), int(port)


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


def wait_port_free(job: Job, host: Host) -> None:
    for _ in range(POLL_ATTEMPTS):
        listening = host.run(["lsof", "-nP", f"-iTCP:{job.port}", "-sTCP:LISTEN", "-t"])
        if not listening.stdout.strip():
            return
        host.sleep(POLL_INTERVAL)
    raise Failed(f"port {job.port} still listens after the bootout of {job.service}")


def bootout(job: Job, host: Host) -> None:
    stop_job(job, host)
    wait_port_free(job, host)


def bootstrap(job: Job, host: Host) -> None:
    launchctl(host, "bootstrap", job.domain, str(job.plist))


def snapshot(job: Job, host: Host, into: Path) -> Path:
    """A verified realm snapshot taken by the installed daemon (RECOVERY.md)."""
    result = host.run(
        [str(job.daemon), "--state-root", str(job.state_root), "snapshot", "--into", str(into)]
    )
    lines = result.stdout.strip().splitlines()
    if result.returncode != 0 or not lines or not Path(lines[-1]).is_file():
        raise Failed(
            f"the snapshot of {job.state_root} failed (exit {result.returncode}): "
            f"{result.stderr.strip()}"
        )
    return Path(lines[-1])


def read_back(job: Job, host: Host, realm_id: str) -> dict:
    """Wait until the started daemon answers realm-get for the expected realm."""
    kontor = job.install_dir / "kontor"
    for _ in range(POLL_ATTEMPTS):
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
) -> dict:
    for name in BINARIES:
        if not (candidates / name).is_file():
            raise Failed(f"{candidates / name} is missing")
    after = {name: digest(candidates / name) for name in BINARIES}
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
            "databaseRestored": False,
        }
        try:
            if started:
                bootout(job, host)
                rollback["newJobStopped"] = True
            if installing:
                restore(job, previous)
                rollback["oldBinariesRestored"] = True
            if stopped:
                bootstrap(job, host)
                rollback["oldJobRestarted"] = True
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
        "--plist", required=True, type=Path, help="the job's plist; its Label must be --label"
    )
    parser.add_argument(
        "--production-deploy",
        metavar="JIRA-KEY",
        help=f"declare a production deploy of {PRODUCTION_LABEL} under this Jira key",
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
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None, host: Host | None = None) -> int:
    arguments = parse_arguments(argv)
    host = host or real_host()
    try:
        job = resolve_job(
            arguments.label, arguments.domain, arguments.plist, arguments.production_deploy, host
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
            )
            print(json.dumps(receipt, indent=2, sort_keys=True))
    except (Failed, OSError, subprocess.SubprocessError) as error:
        print(f"kontor-deploy: failed: {error}", file=sys.stderr)
        return EXIT_FAILED
    return 0


if __name__ == "__main__":
    sys.exit(main())
