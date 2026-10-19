# JoyToKey keyboard and mouse mapping plan

The full analysis and implementation plan moved to GitHub on 16 October 2026:
[Feature #97: JoyToKey-style keyboard and mouse output](https://github.com/opensimgear/cockpit/issues/97).
That issue is the canonical source for architecture, scope and acceptance criteria.

| Milestone | Issue |
| --- | --- |
| J0 Windows behavior contract and API spike | [#98](https://github.com/opensimgear/cockpit/issues/98) |
| J1 Portable contracts and deterministic preview | [#99](https://github.com/opensimgear/cockpit/issues/99) |
| J2 Windows worker and scoped lifecycle | [#100](https://github.com/opensimgear/cockpit/issues/100) |
| J3 Unified profile editor | [#101](https://github.com/opensimgear/cockpit/issues/101) |
| J4 Game-scoped sessions and recovery | [#102](https://github.com/opensimgear/cockpit/issues/102) |
| J5 Sequences and extended assignments | [#103](https://github.com/opensimgear/cockpit/issues/103) |
| J6 Windows qualification and enablement | [#104](https://github.com/opensimgear/cockpit/issues/104) |

Track implementation in the [Plan project](https://github.com/orgs/opensimgear/projects/2).
The core release is J0–J4 plus J6; J5 can follow later. The feature integrates with
[unified profiles #79](https://github.com/opensimgear/cockpit/issues/79), using its
clean storage and shared graph/session model without legacy migration.
