-- OpenSimGear extensions for the pinned DCS-BIOS 0.11.7 module API.
-- Setup supplies `configured` from locally detected, source-matched mods.
-- Loaded before BIOS.lua; uses its existing state machine and network export.
local prefix = 'Scripts.DCS-BIOS.'
local AircraftList = require(prefix .. 'lib.AircraftList')
local Module = require(prefix .. 'lib.modules.Module')
local Protocol = require(prefix .. 'lib.Protocol')
local JSON = require(prefix .. 'lib.ext.JSON')
assert(next(Protocol.exportModules) == nil, 'Aircraft extensions must load before DCS-BIOS')

local function append_unique(list, value)
    for _, existing in ipairs(list) do if existing == value then return end end
    list[#list + 1] = value
end

for _, spec in ipairs(configured) do
    -- A removed mod must not continue to advertise an aircraft on the next launch.
    local entry = io.open(spec.path .. '/entry.lua', 'rb')
    if entry then
        entry:close()
        append_unique(AircraftList.ALL_PLAYABLE_AIRCRAFT, spec.name)
        append_unique(AircraftList.CLICKABLE_COCKPIT_AIRCRAFT, spec.name)
        local aircraft = Module:new(spec.name, spec.base, {spec.name})
        if spec.name == 'MH-6J' then
            aircraft.dev0_required = false
            aircraft.documentation = JSON:newObject()
        else
            for _, control in ipairs(spec.inputs) do
                local id, device, command, arg = control.id, control.device, control.command, control.arg
                local category, description = control.category, control.hint
                if control.kind == 'BTN' then
                    aircraft:defineInputOnlySetStatePushButton(id, device, command, category, description)
                    -- Set-state 1 holds the button; 0 releases it, including negative spring actions.
                    aircraft:addInputProcessor(id, function(value)
                        if value == '0' or value == '1' then
                            local target = GetDevice(device)
                            if target then target:performClickableAction(command,
                                value == '1' and control.press or control.release) end
                        end
                    end)
                elseif control.kind == 'LEV' and control.relative then
                    aircraft:defineFixedStepInput(id, device, command,
                        {-math.abs(control.gain), math.abs(control.gain)}, category, description)
                elseif control.kind == 'LEV' then
                    aircraft:definePotentiometer(id, device, command, arg,
                        {control.min, control.max}, category, description)
                else
                    assert(control.kind == 'TUMB')
                    aircraft:defineTumb(id, device, command, arg, control.step,
                        {control.min, control.max}, nil, control.cycle, category, description)
                end
            end
            for _, output in ipairs(spec.outputs) do
                aircraft:defineFloat(output.id, output.arg, {output.min, output.max},
                    'Instruments and indicators', output.parameter .. ' (normalized cockpit argument)')
            end
        end
        if spec.name == 'AH-6J' then
            -- The pinned legacy module has this name but targets AH-6/BlackHawk.
            -- Retain those aliases and commands in memory, with a separate name.
            -- Requiring BIOS.lua then registers the modern module exactly once.
            local key = prefix .. 'lib.modules.aircraft_modules.AH-6J'
            local legacy = require(key)
            legacy.name = 'AH-6-Legacy'
            Protocol.writeNewModule(legacy)
            package.loaded[key] = aircraft
        else
            Protocol.writeNewModule(aircraft)
        end
    end
end
