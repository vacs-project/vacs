# Client release automation

A `vacs-client` release runs without manual steps from the moment the release PR is merged, in
this order, all inside [release-client](../.github/workflows/release-client.yml):

1. release-please creates the tag and a **draft** release carrying the changelog.
2. The tag push builds, signs and attests the bundles on all platforms.
3. The release job checks that every bundle is present and that exactly one draft exists for the
   tag, uploads the bundles and checksums into the draft, and publishes it last.
4. The servers' release catalogs are reloaded so the updater offers the new version at once.
5. The Homebrew cask is bumped.

Server releases do none of this; they are tagged by hand (see the vacs-server changelog).

---

## Draft until complete

The release exists as a draft while the build runs, which takes 25 to 30 minutes. Drafts are not
listed for anonymous users, `releases/latest` keeps pointing at the previous version, and the
server's GitHub catalog skips them, so nobody is offered a release that has no installers yet.

The order also satisfies GitHub's immutable releases: assets can only be added while the release is
a draft, and publishing locks assets and tag. A published release can therefore never be repaired;
a broken one needs a new patch version.

Two guards run before anything is published:

- **Every bundle is present.** Each build leg only proves that some bundle exists, so the release
  job checks for exactly one file per expected bundle pattern (deb, rpm, AppImage, NSIS installer,
  both DMGs, both app archives, and each `.sig`).
- **Exactly one draft exists for the tag.** Without the draft, publishing would produce a release
  without its changelog. With two releases on the tag, the action would publish whichever it finds
  first, so stale manual drafts for a version must be deleted before its release PR is merged.

### Reruns

| Situation                         | What to do                                                                                                          |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| A build or the release job failed | Re-run the failed jobs of the tag-push run. The draft is still a draft and gets published on success.               |
| A published release is broken     | Ship a new patch version. Assets and tag are locked.                                                                |
| Release candidate or manual build | `workflow_dispatch` with the version. The run creates or updates a draft and leaves it a draft; publish it by hand. |

---

## Reloading the release catalog

The server's update checker reads releases from the GitHub API and caches them for four hours by
default (`release_cache_ttl`). Without a nudge, a version that has just shipped stays invisible to
the updater for up to that long.

The `reload-releases` job calls `POST /admin/releases/reload` on the server, which repopulates the
catalog and prefetches the updater signatures for the newest release of each channel. It runs for
prereleases too, since the `rc` and `beta` channels serve those.

It then **verifies the result** rather than trusting the `200`, by asking `GET /version/update` what
a client on `0.0.1` would be offered, for every platform the build ships:

```
windows/x86_64/nsis
linux/x86_64/deb
linux/x86_64/rpm
linux/x86_64/appimage
darwin/x86_64/app
darwin/aarch64/app
```

Every one of them must come back with the new version. Reload and check are one retry loop, not two
steps, because the GitHub API can still be reporting the release without its assets right after
publishing, and a release with no assets is dropped from the catalog entirely; only another reload
fixes that.

> [!NOTE]
> Keep the `bundles` list of the [reload-releases](../.github/actions/reload-releases/action.yml)
> action in step with what the build produces. It is deliberately strict: a platform silently
> dropping out of the release is the failure this catches. Windows is `nsis`, not `msi`.

### Servers and configuration

Both deployments are reloaded, as a matrix over the `dev` and `production` environments, taking the
server URL and OIDC audience from the environment-scoped `VACS_SERVER_URL` and `VACS_OIDC_AUDIENCE`
variables:

| Environment  | OIDC subject the server must allow                                  |
| ------------ | ------------------------------------------------------------------- |
| `dev`        | `repo:vacs-project@259820785/vacs@993241353:environment:dev`        |
| `production` | `repo:vacs-project@259820785/vacs@993241353:environment:production` |

The repository uses GitHub's immutable subject claims, so owner and repository carry their numeric
IDs and a rename or a transfer cannot make another repository match.

The dev leg runs with `continue-on-error`, so a dev server that is down cannot fail a production
release. A failed production leg fails the run; the release itself is already published and intact,
so fix the cause and re-run the failed `reload-releases` job, or wait for the cache to expire (4 hours
by default). The endpoint only accepts an OIDC token issued to a job in that environment, so it
cannot be called by hand. A release published by hand is not reloaded either and appears once the
cache expires.

Authentication is a GitHub Actions OIDC token, the same mechanism the `vacs-data` repository uses
for `POST /admin/dataset/reload`. The two endpoints deliberately use **separate** allowed subjects,
so that letting one repository reload the dataset never also lets it reload the release catalog.

Each server's `config.toml` allows only its own environment:

```toml
[admin]
oidc_audience = "https://vacs.network"
oidc_allowed_sub_releases = "repo:vacs-project@259820785/vacs@993241353:environment:production"
```

Leave `oidc_allowed_sub_releases` unset and the endpoint answers `404` to everyone, including a
valid token. The subject comes from the job's GitHub Environment, which is why `reload-releases`
declares one; without it the subject would be the git ref and would change with every release.

Repository configuration: the `dev` and `production` environments must allow the `vacs-client-v*`
tag pattern in their deployment branch policy and carry `VACS_SERVER_URL` and `VACS_OIDC_AUDIENCE`.
