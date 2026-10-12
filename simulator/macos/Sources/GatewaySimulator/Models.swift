import Foundation

struct Settings: Codable, Equatable, Sendable {
    var label: String
    var debounce: Int
    var inverted: Bool

    func validated() throws -> Settings {
        let clean = label.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty, clean.count <= 80, !clean.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
            throw SimulatorError.invalid("Label must contain 1–80 printable characters.")
        }
        guard (0...100).contains(debounce) else { throw SimulatorError.invalid("Debounce must be 0–100 ms.") }
        return Settings(label: clean, debounce: debounce, inverted: inverted)
    }
}

struct Request: Decodable, Sendable {
    let id: String
    let command: String
    let moduleId: String?
    let buttons: [Int]?
    let axes: [Int]?
    let online: Bool?
    let settings: Settings?
}

struct Module: Encodable, Sendable {
    let id: String
    let name: String
    let kind: String
    var online: Bool
    var registered: Bool
    var active = false
    var buttons: [Int] = []
    var axes: [Int]
    let buttonCount: Int
    let axisCount: Int
    let serialNumber: String
    var registryID: String?
    var settings: Settings
    let productID: UInt32

    enum CodingKeys: String, CodingKey {
        case id, name, kind, online, registered, active, buttons, axes, buttonCount, axisCount, serialNumber, registryID, settings
    }

    init(id: String, name: String, kind: String, buttons: Int, axes: Int, productID: UInt32, online: Bool = true, registered: Bool = true) {
        self.id = id; self.name = name; self.kind = kind
        self.online = online; self.registered = registered
        self.buttonCount = buttons; self.axisCount = axes
        self.axes = Array(repeating: 0, count: axes)
        self.serialNumber = "OSG-SIM-\(id)"
        self.productID = productID
        self.settings = Settings(label: name, debounce: 10, inverted: false)
    }

    static let defaults = [
        Module(id: "OSG-SQ-0042", name: "OSG Sequential shifter", kind: "shifter", buttons: 3, axes: 0, productID: 0xf101),
        Module(id: "OSG-PD-0018", name: "OSG Pedal set", kind: "pedals", buttons: 0, axes: 3, productID: 0xf102),
        Module(id: "OSG-BP-0007", name: "OSG Button panel", kind: "panel", buttons: 16, axes: 0, productID: 0xf103),
        Module(id: "OSG-HB-0023", name: "OSG Handbrake", kind: "handbrake", buttons: 0, axes: 1, productID: 0xf104, online: false),
    ]

    static let auxiliary = Module(id: "OSG-BP-0012", name: "OSG Auxiliary panel", kind: "panel", buttons: 8, axes: 0, productID: 0xf105, registered: false)
}

struct Snapshot: Encodable, Sendable {
    let running: Bool
    let needsReconnect: Bool
    let modules: [Module]
}

struct Response: Encodable, Sendable {
    let id: String
    let ok: Bool
    let state: Snapshot
    let error: String?
}

enum SimulatorError: Error, LocalizedError {
    case invalid(String)
    case permission

    var errorDescription: String? {
        switch self {
        case .invalid(let message): message
        case .permission: "macOS denied virtual HID creation. This helper requires Apple-authorized signing and a provisioning profile granting com.apple.developer.hid.virtual.device. An administrator launch alone does not grant this entitlement. No controllers were connected."
        }
    }
}
