# Nx CI and daily native checks

## Local development

Use `pnpm test:affected` to run affected portable checks, tests with coverage,
and builds. It includes local staged, unstaged, and untracked changes when no
explicit head is supplied. Use `pnpm test:all` for all portable targets, or
`pnpm nx run profiles-fe:test` for the Profiles frontend. `pnpm test:graph`
shows the selected tasks; `pnpm nx graph` shows all projects.

The existing pnpm packages and Rust crates are separate Nx projects. Portable
Cockpit tests are grouped into profiles, devices, games, widgets, companions,
and shell projects. The local plugin records their source and fixture inputs,
including source read by the Svelte/VM test harnesses. Changes to shared helpers
select their consumers. A test that renders several sections can consequently
be affected by more than one feature. File tests run serially within each group
to keep the geometry performance assertions reliable while Nx runs groups in
parallel.

`pnpm native:affected` runs affected native targets locally. A running development
Cockpit with CDP enabled is required for `pnpm nx run native-host:native-smoke`
and `pnpm nx run native-host:native-ui`. These targets always execute and never
reuse cached results. Set `OSG_PLAYWRIGHT_MODULE` to an installed Playwright
package when it is not resolvable from the repository.

## Push and pull request gate

`ci.yml` is the fast required-check workflow. It resolves the last successful `ci.yml` run on `main` as the baseline for pushes, or uses the pull request base SHA. The first run and manual dispatch run every project. Subsequent runs invoke `nx affected --target=check,test,build` between that baseline and the current commit. Project test targets own their coverage thresholds so CI does not run a second coverage-only target.

The baseline is selected from successful GitHub Actions workflow runs, not from an expiring Actions cache. If the API lookup fails, CI falls back to a full run. The workflow uses full Git history because Nx needs the baseline commit to calculate affected projects.

When `NX_CLOUD_ACCESS_TOKEN` is configured as a repository or organization Actions secret, Nx uses Nx Cloud for shared task results. Without it, `NX_NO_CLOUD` is true and Nx still uses its local task cache. Do not set `NX_SKIP_NX_CACHE` in CI; that would disable both local and remote caching.

The committed `nxCloudId` identifies the created workspace. Finish linking
`opensimgear/cockpit` in Nx Cloud after these files are pushed, then create a
workspace-scoped CI access token and save it as the `NX_CLOUD_ACCESS_TOKEN`
Actions secret. Keep credentials out of source control. Use `pnpm nx login`
for a developer's personal authentication.

## Daily native and integration gate

`daily.yml` runs at 02:17 UTC and can also be started manually. It finds the most recent successful `daily.yml` run on `main`. If that run has the current commit SHA, the workflow skips native testing and its aggregate gate accepts the idle run. If there is no successful previous daily run, the selection is full. Otherwise Nx calculates native projects affected since that run's SHA. A failed or cancelled selected workflow fails the aggregate gate, so a later daily run retries from the last successful daily SHA.

The native marker projects are `native-host`, `native-controllers`, `native-profiles`, `native-persistence`, `native-mapping`, `native-logic`, `native-mobile`, `native-esp32`, and `native-windows-package`. Their `native` targets are the selection contract: give each target the inputs that can affect its platform tests, then make it run the corresponding integration suite. Native targets must not be cached. The marker projects are separate from frontend projects so ordinary UI changes do not schedule every native platform job.

The daily workflow calls `ci-native.yml` for host integration, controllers, profiles, persistence, mapping, and logic tests. It calls the companion mobile and Windows package workflows only when their marker projects are affected. Those workflows retain their platform setup and test/build commands. The ESP32 workflow remains explicitly disabled at the job level while qualification is deferred; selecting its marker does not re-enable board qualification. The aggregate gate permits the expected skipped ESP32 run, while still failing if that workflow reports a failure or cancellation.

The scheduled workflow checks out and tests the event SHA. It updates its effective baseline only through a successful run, including the final aggregate gate. Failures, cancellations, API lookup failures, and skipped selected jobs therefore do not advance the daily baseline.

## Release workflows

`component-release.yml` remains tag-triggered for `cockpit-v*`, `esp32-companion-v*`, and `mobile-companion-v*`. It owns version validation, release packaging, publishing, and announcement; daily CI does not call it.
