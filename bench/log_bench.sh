#!/usr/bin/env bash
#
# Benchmark harness for `gw log`
#
# Creates a temp repo, builds stacks at a few sizes, and measures:
#   1. Wall clock time for `gw log`
#   2. Number of git subprocess calls
#
# Usage: ./bench/log_bench.sh

set -euo pipefail

GW="${GW:-gw}"

BOLD="\033[1m"
DIM="\033[2m"
CYAN="\033[36m"
YELLOW="\033[33m"
GREEN="\033[32m"
RESET="\033[0m"

BENCH_DIR=""
cleanup() { [ -n "$BENCH_DIR" ] && rm -rf "$BENCH_DIR"; }
trap cleanup EXIT

setup_repo() {
  BENCH_DIR=$(mktemp -d)
  cd "$BENCH_DIR"
  git init -q
  git config user.email "bench@test.com"
  git config user.name "Bench"
  echo "init" > README.md
  git add .
  git commit -q -m "initial"
}

# Create a stack with N branches, each with M commits
create_stack() {
  local name="$1" branches="$2" commits_per="$3"
  $GW stack create "$name" > /dev/null
  for b in $(seq 1 "$branches"); do
    [ "$b" -gt 1 ] && $GW branch create "${name}-b${b}" > /dev/null
    for c in $(seq 1 "$commits_per"); do
      echo "${name}-${b}-${c}" > "f-${name}-${b}-${c}.txt"
      git add .
      git commit -q -m "${name} b${b} c${c}"
    done
  done
}

time_log() {
  local label="$1" runs=5 total=0
  $GW log > /dev/null 2>&1  # warmup
  for _ in $(seq 1 $runs); do
    local t
    t=$( { time $GW log > /dev/null 2>&1; } 2>&1 | grep real | awk '{print $2}' | sed 's/[ms]/ /g' | awk '{printf "%.0f", $1*60000+$2*1000}' )
    total=$((total + t))
  done
  local avg=$((total / runs))
  echo -e "  ${GREEN}${label}${RESET}: ${BOLD}${avg}ms${RESET}"
}

count_git() {
  local label="$1"
  local wrapper_dir
  wrapper_dir=$(mktemp -d)
  cat > "$wrapper_dir/git" <<'WRAPPER'
#!/usr/bin/env bash
echo "$@" >> /tmp/gw-bench-git.log
exec /usr/bin/git "$@"
WRAPPER
  chmod +x "$wrapper_dir/git"
  rm -f /tmp/gw-bench-git.log
  PATH="$wrapper_dir:$PATH" $GW log > /dev/null 2>&1
  local count
  count=$(wc -l < /tmp/gw-bench-git.log | tr -d ' ')
  echo -e "  ${YELLOW}git calls${RESET}: ${BOLD}${count}${RESET}"
  # Top commands
  echo -e "  ${DIM}$(awk '{print $1}' /tmp/gw-bench-git.log | sort | uniq -c | sort -rn | head -5 | tr '\n' '  ')${RESET}"
  rm -f /tmp/gw-bench-git.log
  rm -rf "$wrapper_dir"
}

# ── Scenario 1: small (1 stack, 3 branches, 2 commits) ──
echo -e "\n${BOLD}${CYAN}── Small: 1 stack, 3 branches, 2 commits each ──${RESET}"
setup_repo
create_stack "feat" 3 2
git checkout -q main
time_log "gw log"
count_git "gw log"
rm -rf "$BENCH_DIR"

# ── Scenario 2: medium (3 stacks, 3 branches, 3 commits) ──
echo -e "\n${BOLD}${CYAN}── Medium: 3 stacks x 3 branches x 3 commits ──${RESET}"
setup_repo
for s in 1 2 3; do
  git checkout -q main
  create_stack "s${s}" 3 3
done
git checkout -q main
time_log "gw log"
count_git "gw log"
rm -rf "$BENCH_DIR"

# ── Scenario 3: large (5 stacks, 5 branches, 3 commits) ──
echo -e "\n${BOLD}${CYAN}── Large: 5 stacks x 5 branches x 3 commits ──${RESET}"
setup_repo
for s in 1 2 3 4 5; do
  git checkout -q main
  create_stack "s${s}" 5 3
done
git checkout -q main
time_log "gw log"
count_git "gw log"

echo -e "\n${BOLD}Done.${RESET}"
