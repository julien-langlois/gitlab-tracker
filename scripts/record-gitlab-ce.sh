#!/usr/bin/env bash
# Records a GitLab CE version for the contract tests
# (gitlab-tracker-gitlab/tests/contract.rs): starts it in Docker with a runner,
# fills a project the way a team uses one (environment branches, an MR merged
# then cherry-picked, an open MR under discussion, a draft, labels, a milestone,
# a deploy freeze, pipelines), runs the recorder against it, and leaves the
# fixture in gitlab-tracker-gitlab/tests/fixtures/gitlab-ce-<version>/.
#
#   scripts/record-gitlab-ce.sh 18.0.0-ce.0      # any tag of gitlab/gitlab-ce
#
# Needs Docker, curl and jq, and about 6 GB of free memory: GitLab takes several
# minutes to start. The "Record GitLab CE" workflow runs it on a GitHub runner.
set -euo pipefail

version=${1:?usage: $0 <gitlab-ce image tag, e.g. 18.0.0-ce.0 or latest>}
port=${GT_PORT:-8929}
net=gt-record
gitlab=gt-gitlab
runner=gt-runner
url="http://localhost:$port"
token=glpat-contract-recording-0001
root=$(cd "$(dirname "$0")/.." && pwd)

cleanup() {
    docker rm -f "$gitlab" "$runner" > /dev/null 2>&1 || true
    docker network rm "$net" > /dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

echo "Starting gitlab/gitlab-ce:$version…"
docker network create "$net" > /dev/null
docker run -d --name "$gitlab" --network "$net" --hostname gitlab -p "$port:80" \
    --shm-size 256m \
    -e GITLAB_OMNIBUS_CONFIG="external_url 'http://gitlab'; \
        prometheus_monitoring['enable'] = false; \
        puma['worker_processes'] = 0; \
        sidekiq['concurrency'] = 5; \
        gitlab_rails['initial_root_password'] = 'contract-recording-only-123'" \
    "gitlab/gitlab-ce:$version" > /dev/null

# Ready when the API answers; the first boot takes minutes.
for _ in $(seq 1 120); do
    code=$(curl -s -o /dev/null -w '%{http_code}' "$url/api/v4/version" || true)
    [ "$code" = 401 ] && break
    sleep 10
done
[ "$code" = 401 ] || { echo "GitLab did not start" >&2; docker logs --tail 100 "$gitlab"; exit 1; }

echo "Creating the root token…"
docker exec "$gitlab" gitlab-rails runner "
  t = User.find_by_username('root').personal_access_tokens.create!(
    name: 'contract-recording', scopes: ['api', 'read_api'], expires_at: 7.days.from_now)
  t.set_token('$token'); t.save!" > /dev/null

api() {
    local method=$1 path=$2
    shift 2
    curl -sf -X "$method" -H "PRIVATE-TOKEN: $token" "$url/api/v4$path" "$@"
}
gitlab_version=$(api GET /version | jq -r .version)
echo "GitLab $gitlab_version"

echo "A runner, so the commits get pipelines…"
runner_token=$(api POST /user/runners -d runner_type=instance_type -d run_untagged=true | jq -r .token)
docker run -d --name "$runner" --network "$net" gitlab/gitlab-runner:latest > /dev/null
docker exec "$runner" gitlab-runner register --non-interactive --url http://gitlab \
    --token "$runner_token" --executor shell > /dev/null

echo "The project…"
project=$(api POST /projects -d name=tracked -d initialize_with_readme=true -d default_branch=main | jq -r .id)
commit() { # branch, file, message
    api POST "/projects/$project/repository/commits" -H 'Content-Type: application/json' \
        -d "$(jq -n --arg b "$1" --arg f "$2" --arg m "$3" \
            '{branch: $b, commit_message: $m, actions: [{action: "create", file_path: $f, content: ($m + "\n")}]}')" \
        | jq -r .id
}
api POST "/projects/$project/repository/commits" -H 'Content-Type: application/json' -d "$(jq -n '{
    branch: "main", commit_message: "ci: a pipeline for every commit",
    actions: [{action: "create", file_path: ".gitlab-ci.yml",
      content: "stages: [test, build]\nunit:\n  stage: test\n  script: echo ok\nbuild:\n  stage: build\n  script: echo built\n"}]}')" > /dev/null
for b in staging production; do
    api POST "/projects/$project/repository/branches" -d branch=$b -d ref=main > /dev/null
done
for l in "bug:#d9534f" "feature:#428bca"; do
    api POST "/projects/$project/labels" -d name="${l%%:*}" --data-urlencode color="${l#*:}" > /dev/null
done
milestone=$(api POST "/projects/$project/milestones" -d title=v1.0 \
    -d due_date="$(date -u -d '+14 days' +%F 2> /dev/null || date -u -v+14d +%F)" | jq -r .id)
api POST "/projects/$project/freeze_periods" -d freeze_start='0 23 * * 5' \
    -d freeze_end='0 7 * * 1' -d cron_timezone=UTC > /dev/null || true

mr() { # source branch, title, labels → iid
    api POST "/projects/$project/repository/branches" -d branch="$1" -d ref=main > /dev/null
    commit "$1" "$1.txt" "$2" > /dev/null
    api POST "/projects/$project/merge_requests" -d source_branch="$1" -d target_branch=main \
        --data-urlencode title="$2" -d labels="$3" -d milestone_id="$milestone" | jq -r .iid
}
merged=$(mr fix-login "fix: the login keeps the session" bug)
open=$(mr feat-export "feat: export the board" feature)
draft=$(mr wip-search "Draft: search by label" feature)

echo "Merging !$merged, then copying it to staging…"
for _ in $(seq 1 30); do
    status=$(api GET "/projects/$project/merge_requests/$merged" | jq -r .detailed_merge_status)
    [ "$status" = mergeable ] && break
    sleep 5
done
api POST "/projects/$project/merge_requests/$merged/approve" > /dev/null || true
merge_sha=$(api PUT "/projects/$project/merge_requests/$merged/merge" | jq -r '.merge_commit_sha // .sha')
api POST "/projects/$project/repository/commits/$merge_sha/cherry_pick" -d branch=staging > /dev/null || true

api POST "/projects/$project/merge_requests/$open/discussions" --data-urlencode body="Could the export keep the filters?" > /dev/null

echo "Waiting for the pipelines…"
for _ in $(seq 1 60); do
    running=$(api GET "/projects/$project/pipelines?per_page=100" | jq '[.[] | select(.status == "running" or .status == "pending" or .status == "created")] | length')
    [ "$running" = 0 ] && break
    sleep 5
done

echo "Recording…"
# Named after the version that answered (major.minor), "latest" included: a
# patch release does not change the API, and a fixture must say what it is.
name="gitlab-ce-$(echo "$gitlab_version" | cut -d. -f1,2)"
cd "$root"
GT_RECORD_UPSTREAM="$url" GT_RECORD_TOKEN="$token" GT_RECORD_PROJECT="$project" \
GT_RECORD_MRS="$merged,$open,$draft" GT_RECORD_BRANCHES=main,staging,production \
GT_RECORD_NAME="$name" GT_RECORD_DAYS=3 \
GT_RECORD_INSTANCE="GitLab CE $gitlab_version (gitlab/gitlab-ce:$version), seeded by scripts/record-gitlab-ce.sh" \
    cargo test -p gitlab-tracker-gitlab --test contract record -- --ignored --nocapture
echo "Done: gitlab-tracker-gitlab/tests/fixtures/$name"
