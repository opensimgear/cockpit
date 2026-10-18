-- Developer-only extractor for trusted installed mod sources. Run in Lua 5.1.
-- Set MOD_ROOT, BIOS_ROOT and OUTPUT before loading this file.
package.path = BIOS_ROOT .. '/?.lua;' .. package.path
local JSON = dofile(BIOS_ROOT .. '/lib/ext/JSON.lua')
local loadfile_real = loadfile
local allowed = { ['clickable_defs.lua']=true, ['command_defs.lua']=true,
    ['devices.lua']=true, ['sounds.lua']=true }
LockOn_Options = {script_path=MOD_ROOT .. '/Cockpit/Scripts/', common_script_path='@common/'}
function dofile(path)
    local relative = path:sub(#LockOn_Options.script_path + 1)
    assert(path:sub(1, #LockOn_Options.script_path) == LockOn_Options.script_path and allowed[relative], path)
    return assert(loadfile_real(path))()
end
package.loaded.i_18n = {translate=function(value) return value end}
class_type = {BTN='BTN', TUMB='TUMB', LEV='LEV'}
assert(loadfile_real(LockOn_Options.script_path .. 'clickabledata.lua'))()
local reverse_devices, reverse_commands = {}, {}
for name, id in pairs(devices) do reverse_devices[id] = name end
for _, commands in ipairs({device_commands, EFM_commands}) do
    for name, id in pairs(commands) do reverse_commands[id] = name end
end
local controls = {}
for key, element in pairs(elements) do
    for channel, command in ipairs(element.action) do
        local kind = element.class[channel]
        local arg = element.arg[channel]
        local limits = assert(element.arg_lim[channel] or element.arg_lim[1])
        local id = assert(reverse_commands[command], key)
        local unique = id .. '_' .. tostring(arg)
        local record = controls[unique]
        if record then
            assert(record.kind == 'TUMB' and kind == 'TUMB' and record.device == element.device, unique)
            record.min = math.min(record.min, limits[1])
            record.max = math.max(record.max, limits[2])
        else
            record = {id=id, element=key, hint=element.hint, device=element.device,
                category=assert(reverse_devices[element.device]), command=command, arg=arg,
                kind=kind, min=limits[1], max=limits[2], step=math.abs(element.arg_value[channel]),
                press=element.arg_value[channel], release=element.stop_value and element.stop_value[channel] or 0,
                relative=element.relative and element.relative[channel] or false,
                gain=element.gain and element.gain[channel] or 0.1, cycle=element.cycle or false}
            if kind == 'BTN' then
                assert(not element.stop_action or not element.stop_action[channel] or element.stop_action[channel] == command, unique)
            end
            controls[unique] = record
        end
    end
end
local sorted = {}
for key in pairs(controls) do sorted[#sorted+1]=key end
table.sort(sorted)
local inputs, ids = {}, {}
for _, key in ipairs(sorted) do
    local record = controls[key]
    record.id = record.id:upper()
    assert(not ids[record.id], record.id)
    ids[record.id] = true
    inputs[#inputs+1] = record
end
local gauges = {}
function LoRegisterPanelControls() return setmetatable({}, {__index=function() return 0 end}) end
function CreateGauge(kind)
    local gauge = {kind=kind}
    gauges[#gauges+1] = gauge
    return gauge
end
assert(loadfile_real(LockOn_Options.script_path .. 'mainpanel_init.lua'))()
local outputs = {}
for _, value in ipairs(gauges) do
    local name = assert(value.parameter_name) .. '_' .. tostring(value.arg_number)
    if value.arg_number and value.output then
        local lower, upper = math.huge, -math.huge
        for _, number in ipairs(value.output) do lower=math.min(lower,number); upper=math.max(upper,number) end
        assert(lower < upper, name)
        outputs[#outputs+1] = {id='GAUGE_' .. name:upper(), arg=value.arg_number,
            min=lower, max=upper, parameter=value.parameter_name or name}
    end
end
assert(#outputs == #gauges, 'every gauge must have an exported argument and range')
local unique, deduplicated = {}, {}
for _, output in ipairs(outputs) do
    local old = unique[output.id]
    if old then
        assert(old.arg == output.arg and old.min == output.min and old.max == output.max, output.id .. ': ambiguous gauge range')
    else unique[output.id] = output; deduplicated[#deduplicated+1] = output end
end
outputs = deduplicated
table.sort(outputs, function(a,b) return a.id < b.id end)
local file = assert(io.open(OUTPUT, 'w'))
file:write(JSON:encode_pretty({inputs=inputs, outputs=outputs}))
file:close()
