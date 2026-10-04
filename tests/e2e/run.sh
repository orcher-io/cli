#!/usr/bin/env bash
# Drives every command of the CLI against a real engine, started by the CLI
# itself with `orcher dev start`, and a Python worker from tests/e2e.
#
#   tests/e2e/run.sh <path to the orcher binary> <python with orcher-sdk installed>
#
# Every check asserts an exit status and parses the output (with jq where it
# is JSON), and the script exits non-zero when any check failed. It needs
# Docker and jq, and ports 50051 and 8080 free, or others named in
# E2E_GRPC_PORT and E2E_HTTP_PORT. It removes the engine and its data when it
# ends.

set -uo pipefail

ORCHER=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
PYTHON=$2
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
WORKER_PID=
FAILED=0
PASSED=0
GRPC_PORT=${E2E_GRPC_PORT:-50051}
HTTP_PORT=${E2E_HTTP_PORT:-8080}

export NO_COLOR=1
export ORCHER_SERVER=http://localhost:$GRPC_PORT

cleanup() {
  [ -n "$WORKER_PID" ] && kill "$WORKER_PID" 2>/dev/null
  if [ "$FAILED" -ne 0 ]; then
    echo "::group::engine log"
    "$ORCHER" dev logs --tail 200 2>&1 || true
    echo "::endgroup::"
    echo "::group::worker log"
    cat "$WORK/worker.log" 2>/dev/null || true
    echo "::endgroup::"
  fi
  "$ORCHER" dev stop --delete-data -q >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

pass() { PASSED=$((PASSED + 1)); echo "ok    $1"; }
fail() {
  FAILED=$((FAILED + 1))
  echo "FAIL  $1"
  echo "      exit: $STATUS"
  sed 's/^/      stdout: /' "$WORK/out"
  sed 's/^/      stderr: /' "$WORK/err"
}

# run <args...>: runs orcher, keeping its exit status, stdout and stderr.
run() {
  "$ORCHER" "$@" >"$WORK/out" 2>"$WORK/err" </dev/null
  STATUS=$?
}

# check <name> <expected exit status> <jq or grep test> -- <orcher args...>
#   A test starting with "jq:" is a jq expression that must hold for stdout;
#   "err:" is a string stderr must contain; "out:" one stdout must contain;
#   "-" checks the exit status only.
check() {
  local name=$1 want=$2 test=$3
  shift 4
  run "$@"
  if [ "$STATUS" -ne "$want" ]; then
    fail "$name (wanted exit $want)"
    return
  fi
  case $test in
    -) ;;
    jq:*)
      if ! jq -e "${test#jq:}" "$WORK/out" >/dev/null 2>&1; then
        fail "$name (jq: ${test#jq:})"
        return
      fi
      ;;
    err:*)
      if ! grep -qF -- "${test#err:}" "$WORK/err"; then
        fail "$name (stderr lacks: ${test#err:})"
        return
      fi
      ;;
    out:*)
      if ! grep -qF -- "${test#out:}" "$WORK/out"; then
        fail "$name (stdout lacks: ${test#out:})"
        return
      fi
      ;;
    *)
      fail "$name (the test '$test' is not one this script knows)"
      return
      ;;
  esac
  pass "$name"
}

# wait_for <name> <seconds> <jq test> -- <orcher args...>: retries until it holds.
wait_for() {
  local name=$1 seconds=$2 test=$3
  shift 4
  local deadline=$((SECONDS + seconds))
  while :; do
    run "$@"
    if [ "$STATUS" -eq 0 ] && jq -e "$test" "$WORK/out" >/dev/null 2>&1; then
      pass "$name"
      return
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
      fail "$name (not within ${seconds}s: $test)"
      return
    fi
    sleep 1
  done
}

echo "== basics"
check "version matches Cargo.toml" 0 "out:orcher $(sed -n 's/^version = "\(.*\)"/\1/p' "$HERE/../../Cargo.toml" | head -1)" -- --version
check "bash completion" 0 "out:_orcher()" -- completion bash
check "zsh completion" 0 "out:#compdef orcher" -- completion zsh
check "unknown subcommand is a usage error" 2 - -- workflow frobnicate
check "unreachable engine names the address" 1 "err:cannot reach the engine at http://localhost:1" -- --server localhost:1 workflow list

echo "== dev"
check "status before start" 0 "out:not running" -- dev status
DEV_PORTS=(--grpc-port "$GRPC_PORT" --http-port "$HTTP_PORT")
check "start" 0 - -- dev start "${DEV_PORTS[@]}"
check "status is ready" 0 'jq:.engine.state == "ready" and .engine.grpcPort == '"$GRPC_PORT"' and (.engine.image | endswith(":0.5.5")) and .postgres.state == "ready"' -- dev status -o json
check "start again is a no-op" 0 - -- dev start -q "${DEV_PORTS[@]}"
check "start on other ports while running is refused" 1 "err:already running" -- dev start --grpc-port $((GRPC_PORT + 10)) --http-port $((HTTP_PORT + 10))
check "engine log" 0 "out:INFO" -- dev logs --tail 20
if curl -fsS "http://localhost:$HTTP_PORT/health/ready" >/dev/null; then pass "health/ready answers"; else STATUS=$?; : >"$WORK/out"; : >"$WORK/err"; fail "health/ready answers"; fi

echo "== namespaces"
check "list has default" 0 'jq:any(.[]; .name == "default")' -- namespace list -o json
check "create" 0 'jq:.name == "cli-e2e" and .retentionDays == 3 and .data.team == "cli"' -- namespace create cli-e2e --description "made by the e2e test" --retention-days 3 --data team=cli -o json
check "describe" 0 'jq:.name == "cli-e2e" and .description == "made by the e2e test"' -- namespace describe cli-e2e -o json
check "update" 0 'jq:.description == "updated"' -- namespace update cli-e2e --description updated -o json
check "list by name" 0 "out:cli-e2e" -- namespace list -o name
check "deprecate" 0 'jq:.status == "deprecated"' -- namespace deprecate cli-e2e --yes -o json
check "delete needs --yes without a terminal" 1 "err:--yes" -- namespace delete cli-e2e
check "delete" 0 - -- namespace delete cli-e2e --yes
check "describe a missing namespace fails" 1 - -- namespace describe no-such-namespace

echo "== worker"
"$PYTHON" "$HERE/worker.py" >"$WORK/worker.log" 2>&1 &
WORKER_PID=$!

echo "== start and result"
check "start --wait prints the result" 0 'jq:.greeting == "hello, Ada"' -- workflow start hello --task-queue cli-e2e --id hello-1 --input '"Ada"' --wait --timeout 60s
check "start prints ids as json" 0 'jq:.workflowId == "hello-2" and (.executionId | length) > 0' -- workflow start hello --task-queue cli-e2e --id hello-2 --input '"Grace"' -o json
check "result waits for it" 0 'jq:.status == "COMPLETED" and .result.greeting == "hello, Grace"' -- workflow result hello-2 -o json --timeout 60
echo '"Linus"' >"$WORK/input.json"
check "input from a file" 0 'jq:.greeting == "hello, Linus"' -- workflow start hello --task-queue cli-e2e --id hello-3 --input-file "$WORK/input.json" --wait
check "invalid input is refused" 1 "err:--input is not valid JSON" -- workflow start hello --task-queue cli-e2e --input '{oops'
check "-o name prints the id" 0 "out:hello-4" -- workflow start hello --task-queue cli-e2e --id hello-4 --input '"Ken"' -o name
check "a failed workflow exits 1 with its error" 1 "err:this workflow always fails" -- workflow start broken --task-queue cli-e2e --id broken-1 --wait --timeout 60
check "result of a missing workflow fails" 1 "err:not found" -- workflow result no-such-workflow --timeout 5

echo "== inspect"
check "describe" 0 'jq:.workflowId == "hello-1" and .status == "COMPLETED" and .type == "hello" and .taskQueue == "cli-e2e" and .result.greeting == "hello, Ada"' -- workflow describe hello-1 -o json
check "describe as a table" 0 "out:hello, Ada" -- workflow describe hello-1
check "describe a missing workflow fails" 1 "err:not found" -- workflow describe no-such-workflow
check "list" 0 'jq:[.[] | .workflowId] | contains(["hello-1", "hello-2", "broken-1"])' -- workflow list -o json
check "list by status" 0 'jq:length > 0 and all(.[]; .status == "FAILED")' -- workflow list --status failed -o json
check "list by type" 0 'jq:length >= 4 and all(.[]; .type == "hello")' -- workflow list --type hello -o json
check "list by query" 0 'jq:length > 0 and all(.[]; .type == "broken")' -- workflow list --query "type = 'broken'" -o json
check "list rejects an unknown status" 1 "err:unknown status" -- workflow list --status sleepy
check "list as a table" 0 "out:hello-1" -- workflow list
check "history" 0 'jq:(.[0].type == "WORKFLOW_EXECUTION_STARTED") and (.[-1].type == "WORKFLOW_EXECUTION_COMPLETED") and any(.[]; .type == "TASK_COMPLETED")' -- workflow history hello-1 -o json
check "tasks" 0 'jq:length == 1 and .[0].type == "greet" and .[0].attempt >= 1' -- workflow tasks hello-1 -o json
check "logs" 0 "out:execution started" -- logs hello-1
check "logs as json lines" 0 - -- logs hello-1 -o json
if jq -se 'length > 0 and all(.[]; has("message") and has("level"))' "$WORK/out" >/dev/null; then pass "log lines parse"; else fail "log lines parse"; fi
check "logs --follow of a finished workflow returns" 0 "out:execution started" -- logs hello-1 --follow

echo "== events"
check "start a workflow that waits for an event" 0 - -- workflow start approval --task-queue cli-e2e --id approval-1
wait_for "it is waiting" 30 '.status == "RUNNING"' -- workflow describe approval-1 -o json
"$ORCHER" logs approval-1 --follow >"$WORK/follow.out" 2>&1 &
FOLLOW_PID=$!
check "send the event" 0 "out:Event 'approve' sent" -- workflow event approval-1 approve --payload '{"by": "ci"}'
check "event with invalid json is refused" 1 "err:--payload is not valid JSON" -- workflow event approval-1 approve --payload '{oops'
check "the workflow returns the payload" 0 'jq:.approved.by == "ci"' -- workflow result approval-1 --timeout 60
FOLLOW_STATUS=0
for _ in $(seq 1 30); do kill -0 "$FOLLOW_PID" 2>/dev/null || break; sleep 1; done
if kill -0 "$FOLLOW_PID" 2>/dev/null; then
  kill "$FOLLOW_PID"; STATUS=124; cp "$WORK/follow.out" "$WORK/out"; : >"$WORK/err"
  fail "logs --follow ends when the workflow does"
else
  wait "$FOLLOW_PID"; FOLLOW_STATUS=$?
  if [ "$FOLLOW_STATUS" -eq 0 ] && grep -q "completed" "$WORK/follow.out"; then
    pass "logs --follow ends when the workflow does"
  else
    STATUS=$FOLLOW_STATUS; cp "$WORK/follow.out" "$WORK/out"; : >"$WORK/err"
    fail "logs --follow ends when the workflow does"
  fi
fi

echo "== cancel and terminate"
check "start a sleeper" 0 - -- workflow start sleeper --task-queue cli-e2e --id sleeper-1
wait_for "it is sleeping" 30 '.status == "RUNNING" and .pendingTimers == 1' -- workflow describe sleeper-1 -o json
check "result times out" 1 "err:did not finish within 2s" -- workflow result sleeper-1 --timeout 2
check "cancel needs --yes without a terminal" 1 "err:--yes" -- workflow cancel sleeper-1
check "cancel" 0 "out:Cancellation of 'sleeper-1' requested" -- workflow cancel sleeper-1 --yes --reason "e2e"
check "result of a canceled workflow exits 1" 1 "err:ended CANCELED" -- workflow result sleeper-1 --timeout 30
check "describe shows it canceled" 0 'jq:.status == "CANCELED"' -- workflow describe sleeper-1 -o json
check "start another sleeper" 0 - -- workflow start sleeper --task-queue cli-e2e --id sleeper-2
wait_for "it is sleeping" 30 '.status == "RUNNING"' -- workflow describe sleeper-2 -o json
check "terminate" 0 "out:terminated" -- workflow terminate sleeper-2 --yes
check "result of a terminated workflow exits 1" 1 "err:ended TERMINATED" -- workflow result sleeper-2 --timeout 30
check "cancel a missing workflow fails" 1 - -- workflow cancel no-such-workflow --yes

echo "== restart keeps data"
kill "$WORKER_PID" 2>/dev/null; WORKER_PID=
check "stop" 0 "out:data is kept" -- dev stop
check "status after stop" 0 "out:not running" -- dev status
check "commands fail with the engine down" 1 "err:orcher dev start" -- workflow list
check "start again" 0 - -- dev start -q "${DEV_PORTS[@]}"
check "workflows survived the restart" 0 'jq:.status == "COMPLETED" and .result.greeting == "hello, Ada"' -- workflow describe hello-1 -o json
check "stop and delete the data" 0 "out:data deleted" -- dev stop --delete-data
if docker volume inspect orcher-dev-postgres >/dev/null 2>&1; then STATUS=0; : >"$WORK/out"; : >"$WORK/err"; fail "the data volume is gone"; else pass "the data volume is gone"; fi

echo
echo "$PASSED passed, $FAILED failed"
[ "$FAILED" -eq 0 ]
