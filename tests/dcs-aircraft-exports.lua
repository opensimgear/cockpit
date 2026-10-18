-- Offline test of the actual pinned DCS-BIOS API. Driver supplies paths/config.
local scratch = TEST_ROOT .. '/'
local original_open = io.open
io.open = function(path, mode)
    if (mode or 'r'):find('[wa+]') then
        assert(path:gsub('\\', '/'):sub(1, #scratch) == scratch, 'write outside fixture')
    end
    return original_open(path, mode)
end
os.execute = function() error('No process launch in offline test') end
io.popen = function() error('No process launch in offline test') end
package.preload.socket = function() error('No network in offline test') end
lfs = {writedir=function() return scratch end}
package.path = BIOS_SOURCE .. '/?.lua;' .. package.path
local prefix = 'Scripts.DCS-BIOS.'
local JSON = require(prefix .. 'lib.ext.JSON')
assert(require(prefix .. 'BIOSConfig').version == '0.11.7', 'Tests require pinned DCS-BIOS 0.11.7')
local function read(path)
    local file = assert(io.open(path, 'rb')); local content=file:read('*a'); file:close(); return content
end
local specs = JSON:decode(CONFIG)
local hook = read(REPO .. '/examples/dcs-bios/AircraftExports.lua')
assert(loadstring('local configured = require("Scripts.DCS-BIOS.lib.ext.JSON"):decode([====[' .. CONFIG .. ']====])\n' .. hook))()
local Protocol = require(prefix .. 'lib.Protocol')
local CommonData = require(prefix .. 'lib.modules.common_modules.CommonData')
local MetadataStart = require(prefix .. 'lib.modules.common_modules.MetadataStart')
local MetadataEnd = require(prefix .. 'lib.modules.common_modules.MetadataEnd')
for _, module in ipairs({MetadataStart, MetadataEnd, CommonData,
    require(prefix .. 'lib.modules.aircraft_modules.AH-6J')}) do
    Protocol.writeNewModule(module)
end
Protocol.saveAliases()
local mappings = Protocol.aircraft_names_to_modules()
local calls = {}
local dev0 = {get_argument_value=function(_, arg) return 0.25 end}
GetDevice = function(device)
    if device == 0 then return dev0 end
    return {performClickableAction=function(_, command, value)
        calls[#calls+1] = {device=device, command=command, value=value}
    end}
end
local checked = 0
for _, spec in ipairs(specs) do
    if spec.missing then
        assert(not mappings[spec.name], 'removed mod retained its alias')
    else
        local names = Protocol.aircraftNameToModuleNames[spec.name]
        assert(#names == 2 and ((names[1] == spec.name and names[2] == 'CommonData') or (names[2] == spec.name and names[1] == 'CommonData')), spec.name)
        local aircraft = Protocol.exportModules[spec.name]
        if spec.name == 'MH-6J' then
            assert(next(aircraft.inputProcessors) == nil, 'MH-6J must remain read only')
            assert(JSON:encode(aircraft.documentation) == '{}')
        else
            local count = 0
            for _ in pairs(aircraft.inputProcessors) do count=count+1 end
            assert(count == #spec.inputs, spec.name .. ': missing control')
            for _, control in ipairs(spec.inputs) do
                calls = {}
                local process = assert(aircraft.inputProcessors[control.id], control.id)
                local expected
                if control.kind == 'BTN' then
                    process('1'); process('0'); process('2')
                    assert(#calls == 2 and calls[2].value == control.release, control.id .. ': release')
                    expected=control.press
                elseif control.kind == 'LEV' and control.relative then
                    process('INC'); process('DEC')
                    expected=math.abs(control.gain)
                    assert(#calls == 2 and calls[2].value == -expected, control.id .. ': DEC')
                elseif control.kind == 'LEV' then process('65535'); expected=control.max
                else process('0'); expected=control.min end
                assert(calls[1].device == control.device and calls[1].command == control.command, control.id .. ': dispatch')
                assert(math.abs(calls[1].value - expected) < 0.000001, control.id .. ': value')
                checked=checked+1
            end
            for _, export in ipairs(aircraft.exportHooks) do export(dev0) end
            local documentation = JSON:decode(read(scratch .. 'Scripts/DCS-BIOS/doc/json/' .. spec.name .. '.json'))
            local controls = {}
            for _, category in pairs(documentation) do for id, value in pairs(category) do controls[id]=value end end
            for _, output in ipairs(spec.outputs) do
                assert(controls[output.id] and #controls[output.id].outputs == 1, output.id .. ': output missing')
            end
            assert(#aircraft.exportHooks >= #spec.outputs, 'missing gauge hooks')
        end
    end
end
local modern = false
for _, spec in ipairs(specs) do if spec.name == 'AH-6J' and not spec.missing then modern=true end end
for _, alias in ipairs({'AH-6', 'BlackHawk'}) do
    local expected = modern and 'AH-6-Legacy' or 'AH-6J'
    local found = false
    for _, module in ipairs(mappings[alias]) do if module.name == expected then found=true end end
    assert(found, alias .. ': legacy mapping lost')
end
print('PASS: ' .. checked .. ' exact control dispatches; gauges, aliases, read-only MH-6J and missing-mod guard')
