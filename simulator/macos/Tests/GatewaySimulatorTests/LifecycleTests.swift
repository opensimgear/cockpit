import Foundation
import Testing
@testable import GatewaySimulator

private actor RecordingController: ControllerConnection {
    nonisolated let registryID: String
    private var reports: [Data] = []
    init(registryID: String) { self.registryID = registryID }
    func send(_ module: Module) { reports.append(HIDReport.encode(module)) }
    func recorded() -> [Data] { reports }
}

private actor RecordingFactory {
    private var controllers: [String: [RecordingController]] = [:]
    private var count = 0
    func create(_ module: Module) async -> any ControllerConnection {
        count += 1
        let controller = RecordingController(registryID: "test-\(count)")
        controllers[module.id, default: []].append(controller)
        await controller.send(module)
        return controller
    }
    func createdCount() -> Int { count }
    func latest(_ id: String) -> RecordingController { controllers[id]!.last! }
}

private func request(_ command: String, moduleId: String? = nil, buttons: [Int]? = nil, axes: [Int]? = nil, online: Bool? = nil, settings: Settings? = nil) -> Request {
    Request(id: UUID().uuidString, command: command, moduleId: moduleId, buttons: buttons, axes: axes, online: online, settings: settings)
}

@Test func detachRetainsControllerAndInvalidInputsDoNotMutate() async {
    let factory = RecordingFactory()
    let gateway = Gateway(createController: { await factory.create($0) })
    #expect((await gateway.handle(request("start"))).ok)
    let started = await gateway.snapshot()
    let id = "OSG-PD-0018"
    let registry = started.modules.first(where: { $0.id == id })!.registryID
    #expect((await gateway.handle(request("input", moduleId: id, axes: [100, 200, 65535]))).ok)
    let invalid = await gateway.handle(request("input", moduleId: id, axes: [100, 200, 65536]))
    #expect(!invalid.ok)
    #expect(invalid.state.modules.first(where: { $0.id == id })!.axes == [100, 200, 65535])
    let detached = await gateway.handle(request("presence", moduleId: id, online: false))
    let module = detached.state.modules.first(where: { $0.id == id })!
    #expect(detached.ok && module.active && !module.online)
    #expect(module.registryID == registry)
    #expect(await factory.createdCount() == 4)
    let controller = await factory.latest(id)
    #expect(await controller.recorded().last == Data(repeating: 0, count: 6))
    #expect(!(await gateway.handle(request("input", moduleId: id, axes: [1, 2, 3]))).ok)
    #expect((await gateway.handle(request("presence", moduleId: id, online: true))).state.modules.first(where: { $0.id == id })!.registryID == registry)

    #expect((await gateway.handle(request("register"))).ok)
    #expect(await factory.createdCount() == 4)
    let reconnected = await gateway.handle(request("reconnect"))
    #expect(reconnected.ok && reconnected.state.running && !reconnected.state.needsReconnect)
    #expect(reconnected.state.modules.count == 5)
    #expect(reconnected.state.modules.allSatisfy { $0.active && $0.registered })
    #expect(await factory.createdCount() == 9)
    await gateway.stop()
}

@Test func finalReleaseSurvivesDebounceAndStopAlwaysNeutralizes() async throws {
    let factory = RecordingFactory()
    let gateway = Gateway(createController: { await factory.create($0) })
    #expect((await gateway.handle(request("start"))).ok)
    let id = "OSG-SQ-0042"
    #expect((await gateway.handle(request("configure", moduleId: id, settings: Settings(label: "Shifter", debounce: 100, inverted: false)))).ok)
    let controller = await factory.latest(id)
    let press = Task { await gateway.handle(request("input", moduleId: id, buttons: [1])) }
    try await Task.sleep(for: .milliseconds(10))
    let release = await gateway.handle(request("input", moduleId: id, buttons: []))
    #expect(release.ok)
    #expect((await press.value).ok)
    #expect(await controller.recorded().last == Data([0]))
    #expect(!((await controller.recorded()).contains(Data([1]))))
    let pedals = "OSG-PD-0018"
    #expect((await gateway.handle(request("configure", moduleId: pedals, settings: Settings(label: "Pedals", debounce: 0, inverted: true)))).ok)
    let pedalController = await factory.latest(pedals)
    await gateway.stop()
    #expect(await pedalController.recorded().last == Data(repeating: 0, count: 6))
    #expect(!(await gateway.snapshot()).running)
}
