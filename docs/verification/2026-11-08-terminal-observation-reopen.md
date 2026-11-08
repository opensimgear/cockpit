# Terminal observation reopen

A terminal transport error withdrew the previous snapshot but left the
failed exact-topic entry cached while any listener stayed attached.
Reopening a view on the same shared client replayed the terminal state
without establishing a subscription. Both the native WebView and Edge
browser-dev reproduced the failure after a real delivery's receipt failed.

The first new listener after a terminal error now opens one replacement
attempt and transfers the retained listeners. The replacement starts
without a token or baseline. Old callbacks and receipt failures stay
fenced to the retired attempt. Listeners can unsubscribe across the
replacement, and the last listener releases the current subscription.
An ordinary protocol loss still waits for a fresh baseline on the existing
subscription. The client opens no retry while demand stays unchanged.

## Running Cockpit

The fixture used private databases and real subscriptions to theme and
progress preferences. After both baseline receipts, it rejected one
theme receipt at the transport boundary. The old theme subscription
retired and the progress topic stayed ready. A second theme commit ran
while the topic was terminal. New demand delivered one fresh snapshot of
that revision to both theme listeners. Both theme subscriptions retired
on each transport. Browser-dev reused the same multiplexed connection
while the progress reader was present.

A separate browser-dev test stopped the native gateway child owned by
its private Vite process. It observed the terminal failure, reopened on
new demand, and verified that both listeners received the same baseline
from the new gateway session. The fixture observed exactly two physical
connections across the exit and recovery.

Native executable SHA-256
`caac4671b83f2d04168c4a8be865aa53b2483ac930de7f88d0bb6e72928deed8`.
Browser-dev gateway SHA-256
`f6dba6202283c968de62b69a87d9699ae593fec314bf8439a70e410fd85e6fcb`.
Raw logs are under `.research/reactive/observation-terminal-reopen/`.

## Local checks

The 33 stream tests passed. The changed shared client reached 98.40%
line, 98.11% branch and 100% function coverage. The portable regressions
also exercised a delayed failed receipt and synchronous re-registration
during terminal error delivery.

All 1,705 portal tests passed. The production frontend build and Svelte
check passed. The broader native and browser-dev navigation fixtures
also passed, including existing topic retirement and reopen checks.
