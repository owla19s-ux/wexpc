#!/usr/bin/env bash
# Dry-run by default. Execute only with --delete.
# Requires a complete Git clone and push permission to origin.
set -euo pipefail
mode="${1:---dry-run}"
if [[ "$mode" != "--dry-run" && "$mode" != "--delete" ]]; then
  echo "Usage: $0 [--dry-run|--delete]" >&2
  exit 2
fi
git rev-parse --is-inside-work-tree >/dev/null
origin_url="$(git remote get-url origin)"
case "$origin_url" in
  *github.com:owla19s-ux/wexpc.git|*github.com/owla19s-ux/wexpc.git|*github.com/owla19s-ux/wexpc)
    ;;
  *)
    echo "Refusing to run: origin is not owla19s-ux/wexpc ($origin_url)" >&2
    exit 1 ;;
esac
git fetch origin --prune '+refs/heads/*:refs/remotes/origin/*'
main_ref=refs/remotes/origin/main
git rev-parse --verify "$main_ref" >/dev/null
count=0
while IFS= read -r branch; do
  [[ -n "$branch" && "$branch" != main && "$branch" != HEAD ]] || continue
  # PR #7: independent scope-guard prototype; preserve even if main later catches up.
  [[ "$branch" != "feat/pc-scope-overlap-pure-2026-10-25" ]] || continue
  local_ref="refs/remotes/origin/$branch"
  if ! git merge-base --is-ancestor "$local_ref" "$main_ref"; then
    continue
  fi
  local_sha="$(git rev-parse "$local_ref")"
  remote_sha="$(git ls-remote --heads origin "refs/heads/$branch" | cut -f1)"
  [[ -n "$remote_sha" && "$local_sha" == "$remote_sha" ]] || {
    echo "SKIP updated/missing remote: $branch" >&2
    continue
  }
  printf '%s %s\n' "$mode" "$branch"
  if [[ "$mode" == "--delete" ]]; then
    git push origin --delete "$branch"
  fi
  ((count+=1))
done < <(git for-each-ref --format='%(refname:strip=3)' refs/remotes/origin)
printf 'Matched %d fully merged remote branches.\n' "$count"
