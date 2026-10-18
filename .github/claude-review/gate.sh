#!/usr/bin/env bash
# Turns Claude's structured review into the check result. Exit 1 iff the review
# did not run, `blocking` is non-empty, or a golden in visual/required.txt was not reviewed.
set -euo pipefail

: "${REPO:?}" "${PR_NUMBER:?}" "${HEAD_SHA:?}"
export REVIEWER_LOGIN="${REVIEWER_LOGIN:-github-actions}"
OUT="${STRUCTURED_OUTPUT:-}"
CONTEXT_DIR="${CONTEXT_DIR:-.claude-review}"
SUMMARY_FILE="${GITHUB_STEP_SUMMARY:-/dev/null}"

fail_not_run() {
  echo "::error::claude-review did not run: $1"
  {
    echo "### claude-review did not run"
    echo "$1"
  } >>"$SUMMARY_FILE"
  exit 1
}

if [[ -z "${OUT//[[:space:]]/}" ]]; then
  fail_not_run "the action produced no structured output (skipped or crashed). Rerun with: gh run rerun <run-id> --failed"
fi

# The action validates the output against schema.json, so only parseability is in doubt.
if ! jq -e '.blocking | type == "array"' >/dev/null 2>&1 <<<"$OUT"; then
  fail_not_run "the structured output is not valid JSON."
fi

# Workflow-command values need % and newlines escaped, and property values also : and ,.
esc_data() { local s=$1; s=${s//'%'/%25}; s=${s//$'\r'/%0D}; s=${s//$'\n'/%0A}; printf '%s' "$s"; }
esc_prop() { local s; s=$(esc_data "$1"); s=${s//':'/%3A}; s=${s//','/%2C}; printf '%s' "$s"; }

# A thread that is still blocking must stay open even if Claude also listed it.
open_ids=" $(jq -r '[.blocking[].thread_id // empty] | join(" ")' <<<"$OUT") "
while IFS= read -r id; do
  [[ -z "$id" ]] && continue
  if [[ "$open_ids" == *" $id "* ]]; then
    echo "::warning::not resolving $id: it is also a blocking finding"
    continue
  fi
  opener=$(jq -r --arg id "$id" '[.[] | select(.id == $id)][0].comments[0].author // empty' \
    "$CONTEXT_DIR/threads.json")
  if [[ "$opener" != "$REVIEWER_LOGIN" ]]; then
    echo "::warning::not resolving $id: the reviewer did not open it"
    continue
  fi
  # shellcheck disable=SC2016 # $id is a GraphQL variable
  if ! gh api graphql \
    -f query='mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { thread { id } } }' \
    -f id="$id" >/dev/null; then
    echo "::warning::could not resolve thread $id"
  fi
done < <(jq -r '.resolve_thread_ids[]' <<<"$OUT")

blocking_count=$(jq '.blocking | length' <<<"$OUT")

# A review that skipped a golden does not cover HEAD, so the next run reviews those goldens again.
unreviewed=()
if [[ -f "$CONTEXT_DIR/visual/required.txt" ]]; then
  while IFS= read -r golden; do
    unreviewed+=("$golden")
  done < <(jq -r '.goldens_reviewed[]' <<<"$OUT" | sort -u | comm -23 "$CONTEXT_DIR/visual/required.txt" -)
fi
covered=$HEAD_SHA
((${#unreviewed[@]} == 0)) || covered=$(cat "$CONTEXT_DIR/last_sha" 2>/dev/null || true)
thread_url() {
  jq -r --arg id "$1" '[.[] | select(.id == $id)][0].comments[0].url // empty' "$CONTEXT_DIR/threads.json"
}

body=$(mktemp)
errors=()
{
  echo "<!-- claude-review:covered=$covered -->"
  echo "### 🤖 Claude review of \`${HEAD_SHA:0:7}\`"
  echo
  jq -r '.summary' <<<"$OUT"
  echo
  if ((blocking_count == 0 && ${#unreviewed[@]} == 0)); then
    echo "✅ No blocking findings."
  else
    echo "🔴 **$blocking_count blocking finding(s).** Fix the code, or reply on the thread with a concrete reason, then push. To re-evaluate a reply without a code change, run \`gh run rerun <run-id> --failed\`."
    echo
    for ((i = 0; i < blocking_count; i++)); do
      path=$(jq -r ".blocking[$i].path" <<<"$OUT")
      line=$(jq -r ".blocking[$i].line" <<<"$OUT")
      title=$(jq -r ".blocking[$i].title" <<<"$OUT")
      title=${title//$'\n'/ }
      scenario=$(jq -r ".blocking[$i].failure_scenario" <<<"$OUT")
      tid=$(jq -r ".blocking[$i].thread_id // empty" <<<"$OUT")
      url=""
      [[ -n "$tid" ]] && url=$(thread_url "$tid")
      if [[ -n "$url" ]]; then
        echo "- \`$path:$line\` $title ([thread]($url))"
      else
        echo "- \`$path:$line\` $title"
      fi
      echo "  - $scenario"
      errors+=("::error file=$(esc_prop "$path"),line=$line::$(esc_data "$title")")
    done
  fi
  if ((${#unreviewed[@]} > 0)); then
    echo
    echo "🔴 **${#unreviewed[@]} golden(s) not reviewed.** Rerun with \`gh run rerun <run-id> --failed\`."
    echo
    printf -- "- \`%s\`\n" "${unreviewed[@]}"
    errors+=("::error::$(esc_data "${#unreviewed[@]} golden(s) not reviewed")")
  fi
  echo
  echo "Non-blocking observations: $(jq -r '.nonblocking_count' <<<"$OUT")"
} >"$body"

cat "$body" >>"$SUMMARY_FILE"

# A comment failure must not change the verdict, so it only warns.
existing=$(gh api --paginate "repos/$REPO/issues/$PR_NUMBER/comments" \
  --jq '.[] | select(.user.login == (env.REVIEWER_LOGIN + "[bot]") and (.body | test("<!-- claude-review:(sha|covered)="))) | .id' \
  | tail -n1) || existing=""
if [[ -n "$existing" ]]; then
  gh api -X PATCH "repos/$REPO/issues/comments/$existing" -F body=@"$body" >/dev/null \
    || echo "::warning::could not update the summary comment"
else
  gh api -X POST "repos/$REPO/issues/$PR_NUMBER/comments" -F body=@"$body" >/dev/null \
    || echo "::warning::could not post the summary comment"
fi

# A finding that outlived a reply gets the counter-example on its thread, once per reply,
# so the agent reading the thread sees why its reply did not resolve it.
while IFS=$'\t' read -r tid scenario; do
  last_author=$(jq -r --arg id "$tid" '[.[] | select(.id == $id)][0].comments[-1].author // empty' \
    "$CONTEXT_DIR/threads.json")
  [[ -z "$last_author" || "$last_author" == "$REVIEWER_LOGIN" ]] && continue
  # shellcheck disable=SC2016 # $id and $body are GraphQL variables
  gh api graphql \
    -f query='mutation($id: ID!, $body: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $id, body: $body}) { comment { id } } }' \
    -f id="$tid" -f body="$(printf '**🔴 Still blocking**\n\n%s' "$scenario")" >/dev/null \
    || echo "::warning::could not reply on thread $tid"
done < <(jq -r '.blocking[] | select(.thread_id != null) | [.thread_id, (.failure_scenario | gsub("[\t\n]"; " "))] | @tsv' <<<"$OUT")

[[ ${#errors[@]} -eq 0 ]] || printf '%s\n' "${errors[@]}"

((blocking_count == 0 && ${#unreviewed[@]} == 0))
