import CoreHID
import Foundation

protocol ControllerConnection: Sendable {
    var registryID: String { get }
    func send(_ module: Module) async throws
}

actor ControllerDelegate: HIDVirtualDeviceDelegate {
    private var report: Data
    init(report: Data) { self.report = report }
    func update(_ report: Data) { self.report = report }

    func hidVirtualDevice(_ device: HIDVirtualDevice, receivedSetReportRequestOfType type: HIDReportType, id: HIDReportID?, data: Data) async throws {
        throw HIDDeviceError.unsupported
    }

    func hidVirtualDevice(_ device: HIDVirtualDevice, receivedGetReportRequestOfType type: HIDReportType, id: HIDReportID?, maxSize: Int) async throws -> Data {
        guard type == .input, id == nil, maxSize >= report.count else { throw HIDDeviceError.badArgument }
        return report
    }
}

struct VirtualController: ControllerConnection {
    let device: HIDVirtualDevice
    let delegate: ControllerDelegate
    var registryID: String { String(device.deviceReference.deviceID) }

    static func create(for module: Module) async throws -> VirtualController {
        let descriptor = HIDReport.descriptor(buttonCount: module.buttonCount, axisCount: module.axisCount)
        let properties = HIDVirtualDevice.Properties(
            descriptor: descriptor, vendorID: 0x1209, productID: module.productID,
            transport: .virtual, product: module.settings.label,
            manufacturer: "OpenSimGear Development", modelNumber: module.id,
            versionNumber: 1, serialNumber: module.serialNumber,
            uniqueID: module.serialNumber, locationID: UInt64(module.productID)
        )
        guard let device = HIDVirtualDevice(properties: properties) else { throw SimulatorError.permission }
        let delegate = ControllerDelegate(report: HIDReport.encode(module))
        await device.activate(delegate: delegate)
        let controller = VirtualController(device: device, delegate: delegate)
        try await controller.send(module)
        return controller
    }

    func send(_ module: Module) async throws {
        let report = HIDReport.encode(module)
        try await device.dispatchInputReport(data: report, timestamp: .now)
        await delegate.update(report)
    }
}
