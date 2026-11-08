#!/usr/bin/env bash
# Runs a gate on exactly what is being committed or pushed, never on the working tree, so
# other people's uncommitted files in a shared clone neither fail nor slip past the gate.
#
#   scripts/hook-tree.sh commit <gate...>  # gate the index (what `git commit` records)
#   scripts/hook-tree.sh push <gate...>    # gate every commit being pushed; reads the
#                                          # pre-push refs on stdin (HEAD when there are none)
#
# Each tree is exported into target/hook-tree/ (ignored by git), the gate runs there with
# the shared target/ directory as CARGO_TARGET_DIR, and the export is removed afterwards.
set -euo pipefail
mode=${1:?usage: hook-tree.sh commit|push <gate...>}
shift
(($# > 0)) || { echo "hook-tree: no gate command given" >&2; exit 2; }
root=$(git rev-parse --show-toplevel)
base="$root/target/hook-tree"
mkdir -p "$base"
tree=$(mktemp -d "$base/$mode.XXXXXX")
trap 'rm -rf "$tree"' EXIT

run_gate() {
    local label=$1
    shift
    echo "hook-tree: running '$*' on the $label snapshot" >&2
    (cd "$tree" && CARGO_TARGET_DIR="$root/target" "$@")
}

case "$mode" in
    commit)
        git checkout-index --all --prefix="$tree/"
        run_gate "staged" "$@"
        ;;
    push)
        zero=0000000000000000000000000000000000000000
        commits=()
        if [[ -t 0 ]]; then
            commits=("$(git rev-parse HEAD)")
        else
            while read -r _local_ref local_sha _remote_ref _remote_sha; do
                [[ -n "$local_sha" && "$local_sha" != "$zero" ]] && commits+=("$local_sha")
            done
            ((${#commits[@]} > 0)) || commits=("$(git rev-parse HEAD)")
        fi
        for commit in $(printf '%s\n' "${commits[@]}" | sort -u); do
            find "$tree" -mindepth 1 -delete
            git archive "$commit" | tar -x -C "$tree"
            run_gate "commit ${commit:0:12}" "$@"
        done
        ;;
    *)
        echo "hook-tree: unknown mode $mode (expected commit or push)" >&2
        exit 2
        ;;
esac
