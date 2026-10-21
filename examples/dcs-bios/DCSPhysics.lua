-- Read-only ownship telemetry. The managed setup prepends `fields` and `JSON`.
-- No command, object enumeration, weapon, target or socket receive functions.
if _G.__OSG_DCS_PHYSICS_V1 then return end
_G.__OSG_DCS_PHYSICS_V1 = true
local previousStart, previousStop, previousFrame = LuaExportStart, LuaExportStop, LuaExportAfterNextFrame
local udp, socket, session, sequence, sentAt, lastTime, lastPlane, lastAircraft
local function finite(value)
    return type(value) == 'number' and value == value and value > -math.huge and value < math.huge
end
local function text(value)
    return type(value) == 'string' and #value > 0 and #value <= 256 and not value:find('[%z\1-\31\127]')
end
local function stringValue(value)
    return type(value) == 'string' and #value <= 256 and not value:find('[%z\1-\31\127]')
end
local function call(fn)
    if type(fn) ~= 'function' then return nil end
    local ok, a, b, c = pcall(fn)
    if ok then return a, b, c end
end
local function emit(modelTime, plane, aircraft, allowed, paused, activePause, values, stopped)
    if not udp then return end
    sequence = (sequence + 1) % 4294967296
    local packet = {schemaVersion=1, source='osg-dcs-ownship', session=session, sequence=sequence,
        modelTime=finite(modelTime) and modelTime or 0, planeId=plane, aircraft=aircraft,
        allowed=allowed == true, paused=paused == true, activePause=activePause == true,
        stopped=stopped == true, values=values or JSON:newObject()}
    local ok, encoded = pcall(function() return JSON:encode(packet) end)
    if ok and type(encoded) == 'string' and #encoded <= 16384 then pcall(udp.send, udp, encoded) end
end
local function start()
    if udp then pcall(udp.close, udp) end
    udp = nil
    local ok, library = pcall(require, 'socket')
    if not ok then return end
    socket = library
    local connection = socket.udp()
    if not connection then return end
    connection:settimeout(0)
    if not connection:setpeername('127.0.0.1', 7781) then connection:close(); return end
    udp = connection
    session = string.format('%.6f', socket.gettime()) .. '-' .. tostring({}):gsub('[^%w]', '')
    sequence, sentAt, lastTime, lastPlane, lastAircraft = 0, nil, nil, nil, nil
end
local function stop()
    emit(lastTime, nil, nil, false, false, false, nil, true)
    if udp then pcall(udp.close, udp) end
    udp, lastTime, lastPlane, lastAircraft = nil, nil, nil, nil
end
local function frame()
    if not udp then return end
    local now = socket.gettime()
    if not finite(now) or sentAt and now >= sentAt and now - sentAt < 0.05 then return end
    sentAt = now
    local allowed = call(LoIsOwnshipExportAllowed) == true
    local modelTime = call(LoGetModelTime)
    local paused, activePause = call(LoSimulationOnPause) == true, call(LoSimulationOnActivePause) == true
    local plane, aircraft, self
    if allowed then
        plane = call(LoGetPlayerPlaneId)
        if not finite(plane) or plane <= 0 or plane % 1 ~= 0 then plane = nil end
        self = call(LoGetSelfData)
        aircraft = type(self) == 'table' and self.Name or nil
        if not text(aircraft) then aircraft = nil end
    end
    local advancing = finite(modelTime) and finite(lastTime) and modelTime > lastTime
    local same = plane and aircraft and plane == lastPlane and aircraft == lastAircraft
    local values = JSON:newObject()
    if allowed and not paused and not activePause and advancing and same then
        local cache = {LoGetSelfData={self}, LoGetPlayerPlaneId={plane}, LoGetModelTime={modelTime}}
        for _, field in ipairs(fields) do
            local returns = cache[field.getter]
            if not returns then
                local a, b, c = call(_G[field.getter])
                returns = {a, b, c}
                cache[field.getter] = returns
            end
            local value = returns[field.returnIndex or 1]
            for _, key in ipairs(field.path) do if type(value) == 'table' then value = value[key] else value = nil end end
            if field.asText and (finite(value) or type(value) == 'boolean' or text(value)) then value = tostring(value) end
            if field.type == 'number' and finite(value) and value >= field.minimum and value <= field.maximum
                or field.type == 'integer' and finite(value) and value % 1 == 0 and math.abs(value) <= 9007199254740991
                or field.type == 'boolean' and type(value) == 'boolean'
                or field.type == 'string' and stringValue(value) then
                values[field.id] = value
            end
        end
    end
    emit(modelTime, plane, aircraft, allowed, paused, activePause, values)
    lastTime, lastPlane, lastAircraft = finite(modelTime) and modelTime or nil, plane, aircraft
end
function LuaExportStart(...)
    if type(previousStart) == 'function' then pcall(previousStart, ...) end
    pcall(start)
end
function LuaExportStop(...)
    if type(previousStop) == 'function' then pcall(previousStop, ...) end
    pcall(stop)
end
function LuaExportAfterNextFrame(...)
    if type(previousFrame) == 'function' then pcall(previousFrame, ...) end
    pcall(frame)
end
