"""Exercise the Windows release's pinned helper and Play's child-process contract."""
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def response(phase, action, number=2):
    data = {"game": f"uncharted-{number}", "action": action}
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


def test_package(archive, tmp):
    lock = json.loads((ROOT / "packaging/windows/components.lock.json").read_text())
    stage = tmp / "uncharted-windows"
    with zipfile.ZipFile(archive) as package:
        for name in ("uncharted-universal.exe", "save-relay.exe", "components.lock.json",
                     "licenses/save-relay-LICENSE.txt", *(f"save-relay-manifests/uncharted-{n}.json" for n in range(1, 4))):
            package.extract(f"uncharted-windows/{name}", tmp)
    relay = stage / "save-relay.exe"
    assert json.loads((stage / "components.lock.json").read_text()) == lock
    assert hashlib.sha256(relay.read_bytes()).hexdigest() == lock["save-relay"]["sha256"]
    assert "MIT License" in (stage / "licenses/save-relay-LICENSE.txt").read_text()

    runtime = tmp / "synthetic-runtime.exe"
    subprocess.run(["rustc", str(ROOT / "tests/fixtures/relay_process.rs"), "-o", runtime], check=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith(("BB_", "SAVE_RELAY_"))}
    user = tmp / "user"
    user.mkdir()
    env.update(USERPROFILE=str(tmp), LOCALAPPDATA=str(tmp / "data"), APPDATA=str(tmp / "config"),
               BB_USER_DIR=str(user), BB_RUNTIME=str(runtime), RELAY_FIXTURE=str(tmp))
    (user / "save_slots.json").write_text(json.dumps({"Uncharted 2": {"selected": 3}}))
    play = [stage / "uncharted-universal.exe", "play", "u2"]
    for n in range(1, 4):
        manifest = stage / f"save-relay-manifests/uncharted-{n}.json"
        result = subprocess.run([relay, "pull", f"uncharted-{n}", "--manifest", manifest,
                                 "--saves-dir", user / str(n), "--json"], env=env,
                                capture_output=True, text=True, timeout=15)
        assert result.returncode == 4 and json.loads(result.stdout)["error"]["code"] == "not_set_up", result
    run = subprocess.run(play, env=env, capture_output=True, text=True, timeout=15, check=True)
    assert "needs setup" in run.stdout and len(calls(tmp)) == 1, run

    # Keep the tested package untouched; replace the helper in this temporary extracted copy.
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
                event = json.loads(events.get(timeout=15))
                assert (event["event"], event["version"], event["game"], event["phase"]) == ("save-conflict", 1, "u2", phase)
                assert event["server"]["detail"] == "server chapter" and event["local"]["slot"] == "Revision 11"
                child.stdin.write(choice + "\n")
                child.stdin.flush()
            assert child.wait(timeout=15) == 0
            assert "synthetic runtime stdout" in child.stderr.read()
        finally:
            child.kill()
    rows = calls(tmp)
    assert [row[0] for row in rows] == ["pull", "pull", "run", "push", "push"]
    assert rows[1][-2:] == ["--resolve", "keep-server"] and rows[4][-2:] == ["--resolve", "keep-local"]
    slot = user / "Save slots/Uncharted 2/Slot 3"
    for row in [rows[0], rows[1], rows[3], rows[4]]:
        assert Path(row[row.index("--saves-dir") + 1]) == slot

    for options, answer in (([], ""), (["--relay-prompt", "stdio"], ""),
                            (["--relay-prompt", "stdio"], "cancel\n"),
                            (["--relay-prompt", "stdio"], "local" + " " * 30 + "\n")):
        script(tmp, [pull])
        run = subprocess.run([*play, *options], env=env, input=answer, capture_output=True, text=True, timeout=15)
        assert run.returncode != 0 and len(calls(tmp)) == 1 and "pending" in run.stderr

    script(tmp, [("pull", "offline", 2), None, ("push", "deferred", 2)])
    run = subprocess.run(play, env=env, capture_output=True, text=True, timeout=15, check=True)
    assert "offline" in run.stdout and [r[0] for r in calls(tmp)] == ["pull", "run", "push"]
    script(tmp, [("pull", "pushed", 2), None, ("push", "noop", 2),
                 ("pull", "pulled", 3), None, ("push", "pushed", 3)])
    (tmp / "1.switch").write_text("/app0/big3-ps4_Shipping.elf\n")
    subprocess.run(play, env=env, capture_output=True, text=True, timeout=15, check=True)
    rows = calls(tmp)
    assert [r[0] for r in rows] == ["pull", "run", "push", "pull", "run", "push"]
    assert rows[2][1] == "uncharted-2" and Path(rows[2][5]) == slot
    assert rows[3][1] == "uncharted-3" and Path(rows[3][5]) == user / "Uncharted 3"


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as folder:
        test_package(Path(sys.argv[1]), Path(folder))
    print("Windows release Save Relay contract passed.")
