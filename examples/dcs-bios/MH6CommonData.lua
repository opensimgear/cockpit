-- Load before the unchanged DCS-BIOS 0.11.7 BIOS.lua.
-- Exact MH-6J aircraft identity, CommonData only; no AH-6J cockpit commands.
local prefix = "Scripts.DCS-BIOS."
local AircraftList = require(prefix .. "lib.AircraftList")
local Module = require(prefix .. "lib.modules.Module")
local Protocol = require(prefix .. "lib.Protocol")
local JSON = require(prefix .. "lib.ext.JSON")

assert(next(Protocol.aircraft_names_to_modules()) == nil,
    "MH-6J CommonData must load before DCS-BIOS registers aircraft modules")

local function assert_absent(values)
    for _, current in ipairs(values) do
        assert(current ~= "MH-6J", "MH-6J is already supported; review this extension")
    end
end
assert_absent(AircraftList.ALL_PLAYABLE_AIRCRAFT)
assert_absent(AircraftList.CLICKABLE_COCKPIT_AIRCRAFT)
AircraftList.ALL_PLAYABLE_AIRCRAFT[#AircraftList.ALL_PLAYABLE_AIRCRAFT + 1] = "MH-6J"
AircraftList.CLICKABLE_COCKPIT_AIRCRAFT[#AircraftList.CLICKABLE_COCKPIT_AIRCRAFT + 1] = "MH-6J"

-- No controls, memory allocations or export hooks. This base address emits no
-- data because the empty module has no allocated values.
local aircraft = Module:new("MH-6J", 0x6200, { "MH-6J" })
aircraft.dev0_required = false
-- The pinned JSON writer otherwise serializes an empty Lua table as an array.
aircraft.documentation = JSON:newObject()
Protocol.writeNewModule(aircraft)

return { schema = 1, aircraft = "MH-6J", capability = "common-data-only" }
