import Foundation

actor Gateway {
    private var modules = Module.defaults
    private var controllers: [String: any ControllerConnection] = [:]
    private let createController: @Sendable (Module) async throws -> any ControllerConnection
    private var running = false
    private var transitioning = false
    private var generation: [String: UInt64] = [:]
    private var digitalDeadline: [String: ContinuousClock.Instant] = [:]

    init(createController: @escaping @Sendable (Module) async throws -> any ControllerConnection = { try await VirtualController.create(for: $0) }) {
        self.createController = createController
    }

    func snapshot() -> Snapshot {
        Snapshot(running: running, needsReconnect: modules.contains(where: { !$0.registered }), modules: modules)
    }

    func handle(_ request: Request) async -> Response {
        do {
            guard !request.id.isEmpty, request.id.count <= 100 else { throw SimulatorError.invalid("A request ID of 1–100 characters is required.") }
            if transitioning && request.command != "status" { throw SimulatorError.invalid("Gateway is reconnecting. Retry when the transition completes.") }
            switch request.command {
            case "status": break
            case "start":
                if !running {
                    transitioning = true
                    defer { transitioning = false }
                    try await start()
                }
            case "stop": await stop()
            case "reconnect":
                transitioning = true
                defer { transitioning = false }
                await stopControllers()
                try await start()
            case "register":
                let id = request.moduleId ?? Module.auxiliary.id
                guard id == Module.auxiliary.id else { throw SimulatorError.invalid("Only the built-in auxiliary module can be registered.") }
                guard !modules.contains(where: { $0.id == id }) else { throw SimulatorError.invalid("This module is already registered or awaiting reconnect.") }
                modules.append(Module.auxiliary)
            case "presence":
                let index = try index(for: request.moduleId)
                guard let online = request.online else { throw SimulatorError.invalid("Presence requires an online boolean.") }
                modules[index].online = online
                modules[index].buttons = []; modules[index].axes = Array(repeating: 0, count: modules[index].axisCount)
                invalidate(modules[index].id)
                if let controller = controllers[modules[index].id] { try await controller.send(modules[index]) }
            case "configure":
                let index = try index(for: request.moduleId)
                guard let settings = request.settings else { throw SimulatorError.invalid("Configure requires settings.") }
                modules[index].settings = try settings.validated()
                invalidate(modules[index].id)
                if let controller = controllers[modules[index].id] { try await controller.send(modules[index]) }
            case "input": try await input(request)
            default: throw SimulatorError.invalid("Unknown simulator command: \(request.command).")
            }
            return Response(id: request.id, ok: true, state: snapshot(), error: nil)
        } catch {
            return Response(id: request.id, ok: false, state: snapshot(), error: error.localizedDescription)
        }
    }

    private func index(for id: String?) throws -> Int {
        guard let id, let index = modules.firstIndex(where: { $0.id == id }) else { throw SimulatorError.invalid("Unknown module ID.") }
        return index
    }

    private func invalidate(_ id: String) {
        generation[id, default: 0] &+= 1
        digitalDeadline[id] = nil
    }

    private func input(_ request: Request) async throws {
        let index = try index(for: request.moduleId)
        let id = modules[index].id
        guard running, let controller = controllers[id], modules[index].active else { throw SimulatorError.invalid("Start the gateway and reconnect registered modules before sending HID inputs.") }
        guard modules[index].online else { throw SimulatorError.invalid("Reconnect this module to the simulated CAN chain before sending inputs.") }
        guard request.buttons != nil || request.axes != nil else { throw SimulatorError.invalid("Input requires buttons or axes.") }
        let buttons = request.buttons ?? modules[index].buttons
        let axes = request.axes ?? modules[index].axes
        guard buttons.count <= modules[index].buttonCount, Set(buttons).count == buttons.count, buttons.allSatisfy({ (1...max(1, modules[index].buttonCount)).contains($0) && modules[index].buttonCount > 0 }) else {
            throw SimulatorError.invalid("Buttons must be unique held indices in 1…\(modules[index].buttonCount).")
        }
        guard axes.count == modules[index].axisCount, axes.allSatisfy({ (0...65535).contains($0) }) else {
            throw SimulatorError.invalid("Expected \(modules[index].axisCount) unsigned 16-bit axes in 0…65535.")
        }
        let changedButtons = buttons.sorted() != modules[index].buttons
        modules[index].buttons = buttons.sorted(); modules[index].axes = axes
        generation[id, default: 0] &+= 1
        let current = generation[id]
        if changedButtons && modules[index].settings.debounce > 0 {
            digitalDeadline[id] = ContinuousClock.now.advanced(by: .milliseconds(modules[index].settings.debounce))
        }
        if let deadline = digitalDeadline[id], deadline > ContinuousClock.now {
            try await Task.sleep(until: deadline, clock: .continuous)
        }
        // A newer input, configure, detach, or stop supersedes this delayed edge.
        // The newest request always sends its final state, including releases.
        guard generation[id] == current, running, controllers[id] != nil else { return }
        digitalDeadline[id] = nil
        try await controller.send(modules[index])
    }

    private func start() async throws {
        var pending: [String: any ControllerConnection] = [:]
        do {
            for module in modules {
                pending[module.id] = try await createController(module)
            }
        } catch {
            pending.removeAll()
            controllers.removeAll()
            running = false
            for index in modules.indices { modules[index].active = false; modules[index].registryID = nil }
            throw error
        }
        controllers = pending
        running = true
        for index in modules.indices {
            modules[index].registered = true
            modules[index].active = true
            modules[index].registryID = controllers[modules[index].id]!.registryID
        }
    }

    func stop() async {
        transitioning = true
        defer { transitioning = false }
        await stopControllers()
    }

    private func stopControllers() async {
        running = false
        for index in modules.indices {
            let id = modules[index].id
            invalidate(id)
            modules[index].buttons = []; modules[index].axes = Array(repeating: 0, count: modules[index].axisCount)
            if let controller = controllers[id] {
                var neutral = modules[index]
                neutral.online = false
                try? await controller.send(neutral)
            }
            modules[index].active = false; modules[index].registryID = nil
        }
        controllers.removeAll()
        running = false
    }
}
