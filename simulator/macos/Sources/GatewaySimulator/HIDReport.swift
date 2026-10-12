import Foundation

/// Experimental game-controller reports: packed buttons, byte padding, then
/// unsigned little-endian 16-bit X/Y/Z axes. No report IDs, keyboard, or mouse.
enum HIDReport {
    static func descriptor(buttonCount: Int, axisCount: Int) -> Data {
        precondition((0...32).contains(buttonCount) && (0...6).contains(axisCount) && buttonCount + axisCount > 0)
        var bytes: [UInt8] = [0x05, 0x01, 0x09, 0x04, 0xa1, 0x01] // Joystick application
        if buttonCount > 0 {
            bytes += [0x05, 0x09, 0x19, 0x01, 0x29, UInt8(buttonCount), 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, UInt8(buttonCount), 0x81, 0x02]
            let padding = (8 - buttonCount % 8) % 8
            if padding > 0 { bytes += [0x75, 0x01, 0x95, UInt8(padding), 0x81, 0x03] }
        }
        if axisCount > 0 {
            bytes += [0x05, 0x01]
            for index in 0..<axisCount { bytes += [0x09, UInt8(0x30 + index)] }
            // Four-byte logical maximum avoids interpreting 0xffff as -1.
            bytes += [0x15, 0x00, 0x27, 0xff, 0xff, 0x00, 0x00, 0x75, 0x10, 0x95, UInt8(axisCount), 0x81, 0x02]
        }
        bytes += [0xc0]
        return Data(bytes)
    }

    static func encode(_ module: Module) -> Data {
        var bytes = Array(repeating: UInt8(0), count: (module.buttonCount + 7) / 8)
        if module.online {
            for button in module.buttons {
                let mapped = module.kind == "shifter" && module.settings.inverted ? (button == 1 ? 3 : button == 3 ? 1 : button) : button
                bytes[(mapped - 1) / 8] |= UInt8(1 << ((mapped - 1) % 8))
            }
        }
        for raw in module.axes {
            let value = module.online ? (module.settings.inverted ? 65535 - raw : raw) : 0
            bytes += [UInt8(value & 0xff), UInt8((value >> 8) & 0xff)]
        }
        return Data(bytes)
    }
}
