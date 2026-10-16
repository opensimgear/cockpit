# Simple Panels library

Companions and Panels are separate sections displayed together, with Companions first. Panels shows a tile list, with New panel and import actions. Tiles show a static layout thumbnail, title, page/widget counts and assigned companion count. Clicking a tile opens its detail/editor; Back returns to the list. Unsaved changes are guarded during navigation.

Save panel stores the entity and internally creates the validated transport snapshot. Existing assignments to that panel are refreshed in place across all companions. Companion checkboxes allow one panel to be assigned to several paired companions, or assignments to be removed. Assignment changes retain controller identities and unrelated panels/sessions. The native unassign operation releases the removed source without revoking pairing.

There is no Publish action, revision list, content hash or manual controller instance ID in the panel workflow. Existing saved panels and archives load unchanged. Successful saves compact unused transport snapshots while retaining snapshots pinned by active assignments or captured game profiles. Atomic storage failures preserve previously saved data.

Validation: Svelte check, 24 storage/transport/panel-action tests with coverage gates, 65 native companion-service tests including isolated unassignment, and a Windows native build. UI checks in the actual Windows app cover the stacked sections, tile list and editor navigation. Existing firmware transport validation and automatic neutral-baseline handling remain unchanged.
