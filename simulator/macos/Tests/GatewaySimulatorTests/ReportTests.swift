import Foundation
import Testing
@testable import GatewaySimulator

@Test func joystickReportGoldenVectors() {
    var shifter = Module.defaults[0]
    shifter.buttons = [1, 3]
    #expect(HIDReport.encode(shifter) == Data([0x05]))
    shifter.settings.inverted = true
    shifter.buttons = [1]
    #expect(HIDReport.encode(shifter) == Data([0x04]))
    shifter.buttons = [2]
    #expect(HIDReport.encode(shifter) == Data([0x02]))

    var pedals = Module.defaults[1]
    pedals.axes = [0, 32768, 65535]
    #expect(HIDReport.encode(pedals) == Data([0, 0, 0, 128, 255, 255]))
    pedals.settings.inverted = true
    #expect(HIDReport.encode(pedals) == Data([255, 255, 255, 127, 0, 0]))
    pedals.online = false
    #expect(HIDReport.encode(pedals) == Data(repeating: 0, count: 6))

    var panel = Module.defaults[2]
    panel.buttons = [1, 8, 9, 16]
    #expect(HIDReport.encode(panel) == Data([0x81, 0x81]))
    panel.online = false
    #expect(HIDReport.encode(panel) == Data([0, 0]))
}

@Test func settingsRejectInvalidInput() throws {
    #expect(throws: SimulatorError.self) { try Settings(label: "", debounce: 10, inverted: false).validated() }
    #expect(throws: SimulatorError.self) { try Settings(label: "Panel\nOther", debounce: 10, inverted: false).validated() }
    #expect(throws: SimulatorError.self) { try Settings(label: "Panel", debounce: -1, inverted: false).validated() }
    #expect(throws: SimulatorError.self) { try Settings(label: "Panel", debounce: 101, inverted: false).validated() }
    #expect(try Settings(label: "  Panel  ", debounce: 0, inverted: true).validated().label == "Panel")
}

@Test func inventoryRegistrationRequiresReconnect() async throws {
    let gateway = Gateway()
    let before = await gateway.snapshot()
    #expect(before.modules.count == 4)
    #expect(!before.running)
    #expect(!before.needsReconnect)
    #expect(Set(before.modules.map(\.serialNumber)).count == 4)
    let request = try JSONDecoder().decode(Request.self, from: Data(#"{"id":"register","command":"register","moduleId":"OSG-BP-0012"}"#.utf8))
    let response = await gateway.handle(request)
    #expect(response.ok)
    #expect(response.state.needsReconnect)
    #expect(response.state.modules.count == 5)
    #expect(response.state.modules.last?.registered == false)
    #expect(response.state.modules.last?.active == false)
    #expect(!(await gateway.handle(request)).ok)
}
