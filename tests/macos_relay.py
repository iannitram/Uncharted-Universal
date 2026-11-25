"""Exercise the macOS app's pinned helper and Play's child-process contract against a fake relay.

Usage: python3 tests/macos_relay.py dist/uncharted-macos.zip

The fake relay is the synthetic child tests/fixtures/relay_process.rs. The contract: Play pulls
before the game runs and pushes after it; a conflict asks the player, and the answer is passed
back as --resolve; credentials are the helper's own (Keychain), never a file the app writes.
"""
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]


def response(phase, action, number=2):
    data = {"game": f"uncharted-{number}-slot-{3 if number == 2 else 1}", "action": action}
    if action == "conflict":
        for side, day in (("server", 12), ("local", 11)):
            data[side] = {"device": side, "time": f"2026-11-{day}T00:00:00Z", "revision": day}
            data[f"{side}_card"] = {"place": f"{side} chapter", "playtime_secs": day}
    return {"ok": True, "command": phase, "data": data}


def script(folder, steps):
    (folder / "calls").write_text("")
    for index, step in enumerate(steps):
        if step is None:
            continue
        phase, action, number = step
        (folder / f"{index}.json").write_text(json.dumps(response(phase, action, number)))
        (folder / f"{index}.code").write_text("3" if action == "conflict" else "0")


def calls(folder):
    return [line.split("\t") for line in (folder / "calls").read_text().splitlines()]


APP = "Uncharted Universal.app"


def test_package(archive, tmp):
    lock = json.loads((ROOT / "packaging/macos/components.lock.json").read_text())
    # ditto keeps the modes and the bundle exactly as a download would unpack it.
    subprocess.run(["ditto", "-x", "-k", str(archive), str(tmp)], check=True)
    app = tmp / APP
    macos, resources = app / "Contents/MacOS", app / "Contents/Resources"
    relay = macos / "save-relay"
    assert os.access(relay, os.X_OK)
    assert json.loads((resources / "components.lock.json").read_text()) == lock
    assert hashlib.sha256(relay.read_bytes()).hexdigest() == lock["save-relay"]["sha256"]
    assert "MIT License" in (resources / "licenses/save-relay-LICENSE.txt").read_text()
    for n in range(1, 4):
        assert (macos / f"save-relay-manifests/uncharted-{n}.json").is_file()
        assert (resources / f"save-relay-manifests/uncharted-{n}.json").is_file()
    assert subprocess.run(["lipo", "-archs", str(relay)], capture_output=True, text=True,
                          check=True).stdout.strip() == "arm64"
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)

    # The real helper, with no account: a pull reports "not set up" (exit 4), and Play says it needs
    # setup instead of "no Save Relay in this build". Only where no account can exist (CI, or when
    # asked for): the helper reads the Keychain, and this test must never sync a real account.
    runtime = tmp / "synthetic-runtime"
    subprocess.run(["rustc", str(ROOT / "tests/fixtures/relay_process.rs"), "-o", str(runtime)], check=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith(("BB_", "SAVE_RELAY_"))}
    home = tmp / "home"
    user = tmp / "user"
    user.mkdir()
    home.mkdir()
    env.update(HOME=str(home), BB_USER_DIR=str(user), BB_RUNTIME=str(runtime), RELAY_FIXTURE=str(tmp))
    (user / "save_slots.json").write_text(json.dumps({"Uncharted 2": {"selected": 3}}))
    play = [macos / "uncharted-universal", "play", "u2"]
    if os.environ.get("GITHUB_ACTIONS") or os.environ.get("MACOS_RELAY_REAL_HELPER") == "1":
        for n in range(1, 4):
            result = subprocess.run([relay, "pull", f"uncharted-{n}", "--manifest",
                                     macos / f"save-relay-manifests/uncharted-{n}.json",
                                     "--saves-dir", user / str(n), "--slot", "1", "--json"], env=env,
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode == 4 and json.loads(result.stdout)["error"]["code"] == "not_set_up", result
        run = subprocess.run(play, env=env, capture_output=True, text=True, timeout=30, check=True)
        assert "needs setup" in run.stdout and len(calls(tmp)) == 1, run
        assert "no Save Relay" not in run.stdout + run.stderr and "not in this build" not in run.stdout + run.stderr
    else:
        print("skipped the real helper: set MACOS_RELAY_REAL_HELPER=1 where no account is signed in")

    # Keep the tested package untouched: swap the helper for the fake in this temporary copy only.
    # (Contents/MacOS is sealed by the ad-hoc signature, so the copy is no longer verifiable.)
    shutil.copyfile(runtime, relay)
    pull = ("pull", "conflict", 2)
    push = ("push", "conflict", 2)
    script(tmp, [pull, ("pull", "pulled", 2), None, push, ("push", "pushed", 2)])
    with subprocess.Popen([*play, "--relay-prompt", "stdio"], env=env, stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as child:
        try:
            events = queue.Queue()

            def read_events():
                for line in child.stdout:
                    events.put(line)
            threading.Thread(target=read_events, daemon=True).start()
            for phase, choice in (("pull", "server"), ("push", "local")):
                event = json.loads(events.get(timeout=30))
                assert (event["event"], event["version"], event["game"], event["phase"]) == ("save-conflict", 1, "u2", phase)
                child.stdin.write(choice + "\n")
                child.stdin.flush()
            assert child.wait(timeout=30) == 0
            assert "synthetic runtime stdout" in child.stderr.read()
        finally:
            child.kill()
    rows = calls(tmp)
    # Pull before the game, push after; each conflict answered with --resolve.
    assert [row[0] for row in rows] == ["pull", "pull", "run", "push", "push"]
    assert rows[1][-2:] == ["--resolve", "keep-server"] and rows[4][-2:] == ["--resolve", "keep-local"]
    slot = user / "Save slots/Uncharted 2/Slot 3"
    for row in [rows[0], rows[1], rows[3], rows[4]]:
        assert Path(row[row.index("--saves-dir") + 1]) == slot
        assert row[row.index("--slot") + 1] == "3"
        assert Path(row[row.index("--manifest") + 1]).parent == macos / "save-relay-manifests"

    # An unanswered conflict never starts the game.
    for options, answer in (([], ""), (["--relay-prompt", "stdio"], ""), (["--relay-prompt", "stdio"], "cancel\n")):
        script(tmp, [pull])
        run = subprocess.run([*play, *options], env=env, input=answer, capture_output=True, text=True, timeout=30)
        assert run.returncode != 0 and len(calls(tmp)) == 1 and "pending" in run.stderr

    # Offline: the game still runs on local saves and the push is deferred.
    script(tmp, [("pull", "offline", 2), None, ("push", "deferred", 2)])
    run = subprocess.run(play, env=env, capture_output=True, text=True, timeout=30, check=True)
    assert "offline" in run.stdout and [r[0] for r in calls(tmp)] == ["pull", "run", "push"]

    # No credential file is written by the app side of Play.
    for found in tmp.rglob("*"):
        assert not re.search(r"token|credential|password|secret", found.name, re.I), found


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as folder:
        test_package(Path(sys.argv[1]), Path(folder))
    print("macOS app Save Relay contract passed.")
