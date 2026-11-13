# GitLab token

`gitlab-tracker` only reads from GitLab. Give it a token that can do nothing
else: a [fine-grained personal access token](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens/)
(GitLab 19.2 and later, all tiers) limited to the projects you track, with
**Read** permissions only. If it leaks, it exposes those projects read-only —
not every project your account can reach, and nothing it could change.

- [Create the token](#create-the-token)
- [Permissions](#permissions)
- [Check the token](#check-the-token)
- [One token per instance, replacing a token](#one-token-per-instance-replacing-a-token)
- [Older GitLab (before 19.2)](#older-gitlab-before-192)
- [Keeping this page up to date](#keeping-this-page-up-to-date)

## Create the token

1. Avatar › **Edit profile** › **Access › Personal access tokens**.
2. **Generate token** › **Fine-grained token**. Fill in **Name** (e.g.
   `gitlab-tracker – my-project`) and an **Expiration date** (90 days is a good
   habit).
3. Under **Group and project access**, choose the option that limits the token
   to **selected groups and projects** — not all your projects.
4. Under **Add resource permissions**, give **Read** — and only Read — to the
   resources listed in [Permissions](#permissions).
5. **Generate token**, copy it (it is shown only once), and paste it when the
   app asks for it.

## Permissions

### Group and project tab

Select your project(s), then open **Select permissions** on each resource below
and pick **Read**. Leave every other resource empty.

| Category | Resource | Endpoints the app calls | What it is for |
| :--- | :--- | :--- | :--- |
| CI/CD | **Deployment** | `deployments` | Environments page |
| CI/CD | **Job** | `jobs/:job_id/trace` · GraphQL `CiJob` | Job logs, jobs of the MR pipelines |
| CI/CD | **Pipeline** | `pipelines`, `pipelines/:pipeline_id/jobs` · GraphQL `Pipeline`, `CiStage` | Pipelines and their jobs |
| Project Planning | **Label** | `labels` | Label chips and filters |
| Project Planning | **Work Item** | `milestones`, `merge_requests/:iid/resource_milestone_events` | Milestones, milestone changes in the MR timeline |
| Projects | **Project** | `/projects/:id` · GraphQL `Project` | The project itself (checked when it is added) |
| Repository | **Commit** | `repository/commits`, `repository/commits/:sha`, `…/:sha/refs`, `…/:sha/diff`, `…/:sha/signature` | Branch scans, branches holding a commit, signed commits |
| Repository | **Merge Request** | `merge_requests`, `merge_requests/:iid`, `…/changes`, `…/commits`, `…/versions`, `…/pipelines`, `…/discussions`, `…/approvals`, `…/approval_state` · GraphQL `MergeRequest` | MRs and everything shown about them |
| Repository | **Repository** | `repository/compare`, `repository/merge_base`, `repository/files/:file_path/raw` | Branch comparisons, `CHANGELOG.md` |
| Repository | **Repository Tag** | `repository/tags` | Changelog and releases |

All paths are under `/api/v4/projects/:id/`. GraphQL needs nothing more: GitLab
checks each type a query reads, and the ones above cover the app's query.

Nothing else is needed. In the CI/CD category for instance, only **Deployment**
is: *Catalog Version*, *CI Config*, *CI/CD Setting*, *Cluster…*, *Commit
Status* or *Environment* can stay empty.

### User tab (optional)

| Category | Resource | Endpoint | What it is for |
| :--- | :--- | :--- | :--- |
| System Access | **User** | `GET /user` | Your username, for the "me" views (Today, *To review: me*, *Assigned to me*, review reminders) |

Without it the app still works: set your username yourself in `projects.toml`
and `/user` is never called.

```toml
gitlab_username = "jdoe"   # the handle after @ in GitLab, exactly
```

### Your role still applies

GitLab accepts a request only if the token has the permission **and** your
account may do the same thing on the project. The token never goes beyond your
role (Reporter at least, for job logs and approvals) and sees no other project,
even one your account can open.

## Check the token

The token never appears in the shell history or the process list:

```bash
read -rsp "Token: " T; echo
G=https://gitlab.example.com/api/v4; P=<project id>
for path in "/projects/$P" "/projects/$P/merge_requests" "/projects/$P/pipelines" \
  "/projects/$P/repository/commits" "/projects/$P/repository/tags" "/projects/$P/labels" \
  "/projects/$P/milestones" "/projects/$P/deployments" "/user" "/projects/<another project id>"; do
  printf '%-45s %s\n' "$path" "$(curl -s -o /dev/null -w '%{http_code}' \
    -H @<(printf 'PRIVATE-TOKEN: %s\n' "$T") "$G$path")"
done
unset T
```

Expect `200` everywhere (`403` on `/user` if you skipped the User tab), and
`403`/`404` on the last line: the token cannot see another project. A `403`
elsewhere names the endpoint whose resource is missing in the table above.

## One token per instance, replacing a token

**One token per instance.** The keyring holds one GitLab token per instance URL,
shared by every project tracked on that instance. When you track another project
on the same GitLab, add it to the token's resources (or use a group: a token
scoped to a group covers its projects) rather than creating a second token.

**Replacing a token** (rotation, expiry, a missing permission, switch to
fine-grained): delete the keyring entry, then start the app, which asks for the
new one. Revoke the old token in GitLab afterwards.

```bash
secret-tool clear service gitlab-tracker username 'gitlab_token::https://gitlab.example.com'   # Linux (libsecret-tools)
```

## Older GitLab (before 19.2)

Use a classic personal access token with the single scope `read_api`, or a
[project access token](https://docs.gitlab.com/user/project/settings/project_access_tokens/)
(role Reporter, scope `read_api`), which is also limited to one project.

## Keeping this page up to date

*For contributors.* Any change that adds a GitLab call — REST path or GraphQL
type — updates the [Permissions](#permissions) table in the same commit, and
the short list in the [README](https://github.com/julien-langlois/gitlab-tracker/blob/main/README.md#-which-token-to-create). Look the
endpoint up in GitLab's own tables, which are generated from GitLab's code:

- REST: [Fine-grained permissions for REST API](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens_rest/)
- GraphQL: [Fine-grained permissions for GraphQL API](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens_graphql/)
- Git and other operations: [Fine-grained permissions for other operations](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens_other/)

The heading above a row is the resource name shown in the token form; the
*Action* column is the permission to pick. Calls that write to GitLab would need
more than **Read**: say so here, since the app is read-only today.
