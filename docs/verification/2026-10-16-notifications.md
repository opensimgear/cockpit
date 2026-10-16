# Shared Cockpit notifications

Transient action results and errors use a shared bottom-right toast stack across Cockpit views, including companion provisioning, panels, mappings, integrations, games and settings. Notifications survive view changes, deduplicate identical active messages and cap the stack at five. Messages dismiss automatically (six seconds, twelve for errors) or with an accessible close button. Ongoing device/provisioning status and configuration controls remain in their relevant views.

Panel import and New panel actions share the top-right header, with icons. The empty state has no duplicate create action. Editor actions also have icons.

Validation: Svelte check, frontend production build, notification tests covering deduplication, timeouts, dismissal and bounded queues, and Windows app visual checks.
