#!/bin/sh
# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

# Run ingest in AWS again and again until nothing is waiting (`make backfill`): no
# summary, sharing label, sector or website lookup left for a later run. Use it after
# a large change (a new field, a prompt change, a cleared cache) instead of waiting for
# the daily runs to work through the backlog.
#
# Each run stops starting work before Lambda's limit and saves what it did, so every
# run makes progress. This stops when nothing is waiting, when a run makes no progress
# (failures, missing credentials, the network), when a run fails, or after MAX_RUNS.
#
# It never overlaps the scheduled run: two ingests at once would each save their own
# copy of the caches and lose the other's work. Runs that would come within
# SCHEDULE_GAP_SECS of the schedule wait until it has passed.
#
# Environment: FUNCTION (required, the ingest function's name), AWS_PROFILE,
# AWS_REGION, MAX_RUNS (default 20), RULE (the EventBridge rule; default FUNCTION),
# SCHEDULE_GAP_SECS (default 960, the function's timeout plus a minute).
set -eu
: "${FUNCTION:?set FUNCTION to the name of the ingest function}"
MAX_RUNS=${MAX_RUNS:-20}
RULE=${RULE:-$FUNCTION}
GAP=${SCHEDULE_GAP_SECS:-960}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT INT TERM

# The scheduled run's time of day in seconds after midnight UTC, from a daily
# cron(M H * * ? *) rule; empty for any other schedule, which is not guarded.
schedule=$(aws events describe-rule --name "$RULE" --query ScheduleExpression --output text 2>/dev/null || true)
sched_secs=$(echo "$schedule" | sed -n 's/^cron(\([0-9]\{1,2\}\) \([0-9]\{1,2\}\) \* \* ? \*)$/\2 \1/p' |
  awk '{ print $1 * 3600 + $2 * 60 }')
if [ -z "$sched_secs" ]; then
  echo "warning: cannot read a daily schedule from rule $RULE ('$schedule'); not guarding against an overlap" >&2
fi

# Seconds to wait so a run started now cannot overlap the scheduled one: 0, or until
# GAP seconds after it.
wait_for_schedule() {
  [ -n "$sched_secs" ] || { echo 0; return; }
  now=$(date -u +%s)
  midnight=$((now - now % 86400))
  for day in -86400 0 86400; do
    at=$((midnight + day + sched_secs))
    if [ "$now" -gt $((at - GAP)) ] && [ "$now" -lt $((at + GAP)) ]; then
      echo $((at + GAP - now))
      return
    fi
  done
  echo 0
}

previous=""
run=1
while [ "$run" -le "$MAX_RUNS" ]; do
  pause=$(wait_for_schedule)
  if [ "$pause" -gt 0 ]; then
    echo "waiting ${pause}s for the scheduled run to finish"
    sleep "$pause"
  fi

  # One attempt and a read timeout past the function's own: a retried invoke would
  # start a second, overlapping run.
  started=$(date -u +%s)
  if ! AWS_MAX_ATTEMPTS=1 aws lambda invoke --function-name "$FUNCTION" \
    --cli-read-timeout 960 --cli-binary-format raw-in-base64-out --payload '{}' \
    --query FunctionError --output text "$WORK/out.json" >"$WORK/error.txt"; then
    echo "run $run: the invoke failed" >&2
    exit 1
  fi
  if [ "$(cat "$WORK/error.txt")" != "None" ]; then
    echo "run $run failed: $(cat "$WORK/out.json")" >&2
    exit 1
  fi
  elapsed=$(($(date -u +%s) - started))

  # Waiting work per step (a step that is off, null, has none), then any reason a
  # step gave for not running at all, one per line.
  python3 - "$WORK/out.json" >"$WORK/waiting.txt" <<'EOF'
import json, sys
out = json.load(open(sys.argv[1]))
step = lambda name: out.get(name) or {}
logos = step("logos")
waiting = {
    "summaries": step("summaries").get("waiting") or 0,
    "sharing": step("sharing").get("waiting") or 0,
    "sectors": step("sectors").get("waiting") or 0,
    "logo sites": logos.get("sites_waiting") or 0,
    # Logo checks and lookups not started for time (the lookups are also sites).
    "logos out of time": logos.get("stopped_for_time") or 0,
}
print(sum(waiting.values()))
print(", ".join(f"{k} {v}" for k, v in waiting.items()))
for name in ("summaries", "sharing", "sectors"):
    if step(name).get("skipped_reason"):
        print(f"{name}: {step(name)['skipped_reason']}")
if logos.get("lookup_skipped_reason"):
    print(f"logos: {logos['lookup_skipped_reason']}")
EOF
  total=$(sed -n 1p "$WORK/waiting.txt")
  echo "run $run (${elapsed}s): waiting: $(sed -n 2p "$WORK/waiting.txt")"

  if [ "$total" -eq 0 ]; then
    echo "done: nothing is waiting"
    exit 0
  fi
  if [ -n "$(sed -n '3,$p' "$WORK/waiting.txt")" ]; then
    echo "stopped: work is waiting that no run will do:" >&2
    sed -n '3,$p' "$WORK/waiting.txt" | sed 's/^/  /' >&2
    exit 1
  fi
  if [ -n "$previous" ] && [ "$total" -ge "$previous" ]; then
    echo "stopped: run $run made no progress ($total waiting, $previous before); see the ingest logs" >&2
    exit 1
  fi
  previous=$total
  run=$((run + 1))
done
echo "stopped after $MAX_RUNS runs with $previous waiting; run again to continue" >&2
exit 1
