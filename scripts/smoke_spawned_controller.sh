#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
controller_repo="${JASON_CONTROLLER_REPO:-$(cd "$repo_root/.." && pwd)/jason}"

if [ ! -f "$controller_repo/Cargo.toml" ]; then
  echo "skip: sibling jason controller checkout not found at $controller_repo"
  exit 0
fi

tmp_root="$(mktemp -d)"
controller_pid=""
cleanup() {
  if [ -n "$controller_pid" ]; then
    kill "$controller_pid" >/dev/null 2>&1 || true
    wait "$controller_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_root"
}
trap cleanup EXIT

port="$(
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
endpoint="http://127.0.0.1:$port"
database="$tmp_root/jason.db"
login_state="$tmp_root/login.json"

cargo build --manifest-path "$repo_root/Cargo.toml" --bin jason >/dev/null
cargo build --manifest-path "$controller_repo/Cargo.toml" --bin jason --bin jason-controller >/dev/null

"$controller_repo/target/debug/jason" --database "$database" --dev init --json >/dev/null
"$controller_repo/target/debug/jason-controller" \
  --database "$database" \
  --dev \
  --bind "127.0.0.1:$port" \
  >"$tmp_root/controller.log" 2>&1 &
controller_pid="$!"

for _ in $(seq 1 50); do
  if curl -fsS "$endpoint/v1/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done
curl -fsS "$endpoint/v1/health" >/dev/null

cat >"$login_state" <<EOF
{
  "jason_controller_endpoint": "$endpoint",
  "access_token": "local-controller-token",
  "expires_at": "4102444800",
  "audience": "jason-controller",
  "scopes": ["jason:controller"]
}
EOF

client="$repo_root/target/debug/jason"
"$client" --login-state "$login_state" doctor --json | jq -e '.ok == true' >/dev/null
"$client" --login-state "$login_state" --json status | jq -e '.ok == true' >/dev/null

run_json="$("$client" --login-state "$login_state" --json run --repo mithran-hq/demo --issue 123)"
task_id="$(printf '%s\n' "$run_json" | jq -r '.task.id')"
test -n "$task_id"
test "$task_id" != "null"

"$client" --login-state "$login_state" --json status "$task_id" | jq -e '.task.id == "'"$task_id"'"' >/dev/null
"$client" --login-state "$login_state" --json logs "$task_id" | jq -e '.ok == true and (.logs | length) == 0' >/dev/null
"$client" --login-state "$login_state" --json artifacts "$task_id" | jq -e '.ok == true and (.artifacts | length) == 0' >/dev/null
"$client" --login-state "$login_state" --json cancel "$task_id" | jq -e '.task.state == "cancelled"' >/dev/null
"$client" --login-state "$login_state" --json watch "$task_id" --interval-seconds 1 --timeout-seconds 2 | jq -e 'select(.task.state == "cancelled")' >/dev/null

echo "jason-client spawned-controller smoke passed: $task_id"
