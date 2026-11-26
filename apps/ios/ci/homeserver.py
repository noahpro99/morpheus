#!/usr/bin/env python3
"""A throw-away Synapse for the iOS XCUITests (macOS runners have no docker): `homeserver.py start <dir>`
installs matrix-synapse into <dir>/venv, runs it on :8008 and seeds alice (password `alice-pass-1`) with a DM
from bob, and a server "Test Server" with the channel "general" holding 60 messages ("history 1" ... "history 60")."""
import json, os, subprocess, sys, time, urllib.request

HS = "http://localhost:8008"


def call(method, path, body=None, token=None):
    req = urllib.request.Request(HS + path, method=method, data=json.dumps(body).encode() if body is not None else None)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    with urllib.request.urlopen(req) as r:
        return json.load(r)


def register(name, password):
    return call("POST", "/_matrix/client/v3/register", {"username": name, "password": password, "auth": {"type": "m.login.dummy"}})["access_token"]


def start(root):
    os.makedirs(root, exist_ok=True)
    venv = os.path.join(root, "venv")
    subprocess.check_call([sys.executable, "-m", "venv", venv])
    py = os.path.join(venv, "bin", "python")
    subprocess.check_call([py, "-m", "pip", "install", "-q", "matrix-synapse"])
    key = os.path.join(root, "signing.key")
    subprocess.check_call([py, "-m", "synapse._scripts.generate_signing_key", "-o", key])
    cfg = os.path.join(root, "homeserver.yaml")
    big = "{ per_second: 1000, burst_count: 1000 }"
    open(cfg, "w").write(f"""server_name: localhost
pid_file: {root}/hs.pid
listeners:
  - port: 8008
    bind_addresses: ['127.0.0.1']
    type: http
    tls: false
    resources:
      - names: [client]
database:
  name: sqlite3
  args: {{database: {root}/hs.db}}
media_store_path: {root}/media
signing_key_path: {key}
report_stats: false
suppress_key_server_warning: true
enable_registration: true
enable_registration_without_verification: true
rc_message: {big}
rc_registration: {big}
rc_login:
  address: {big}
  account: {big}
  failed_attempts: {big}
rc_joins:
  local: {big}
  remote: {big}
rc_invites:
  per_room: {big}
  per_user: {big}
rc_room_creation: {big}
""")
    log = open(os.path.join(root, "synapse.log"), "w")
    subprocess.Popen([py, "-m", "synapse.app.homeserver", "-c", cfg], stdout=log, stderr=log, start_new_session=True)
    for _ in range(120):
        try:
            urllib.request.urlopen(HS + "/health", timeout=2)
            break
        except Exception:
            time.sleep(1)
    else:
        sys.exit("synapse did not come up")
    seed()


def seed():
    alice = register("alice", "alice-pass-1")
    bob = register("bob", "bob-pass-1")
    # A DM from bob.
    dm = call("POST", "/_matrix/client/v3/createRoom", {"is_direct": True, "invite": ["@bob:localhost"], "preset": "trusted_private_chat"}, alice)["room_id"]
    call("POST", f"/_matrix/client/v3/join/{dm}", {}, bob)
    call("PUT", "/_matrix/client/v3/user/@alice:localhost/account_data/m.direct", {"@bob:localhost": [dm]}, alice)
    call("PUT", "/_matrix/client/v3/user/@bob:localhost/account_data/m.direct", {"@alice:localhost": [dm]}, bob)
    say(bob, dm, "hello from bob", 0)
    # A server (a space) with a channel.
    space = call("POST", "/_matrix/client/v3/createRoom", {"name": "Test Server", "creation_content": {"type": "m.space"}, "preset": "public_chat"}, alice)["room_id"]
    chan = call("POST", "/_matrix/client/v3/createRoom", {"name": "general", "preset": "public_chat"}, alice)["room_id"]
    call("PUT", f"/_matrix/client/v3/rooms/{space}/state/m.space.child/{chan}", {"via": ["localhost"]}, alice)
    call("PUT", f"/_matrix/client/v3/rooms/{chan}/state/m.space.parent/{space}", {"via": ["localhost"], "canonical": True}, alice)
    for i in range(1, 61):
        say(alice, chan, f"history {i}", i)
    print("seeded")


def say(token, room, text, n):
    call("PUT", f"/_matrix/client/v3/rooms/{room}/send/m.room.message/seed{n}-{abs(hash(room))}", {"msgtype": "m.text", "body": text}, token)


if __name__ == "__main__":
    start(sys.argv[2]) if sys.argv[1] == "start" else sys.exit("usage: homeserver.py start <dir>")
