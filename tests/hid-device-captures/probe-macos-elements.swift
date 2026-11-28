import CryptoKit
import Foundation
import IOKit
import IOKit.hid

struct Arguments {
    var registryID: UInt64?
    var listOnly = false
}

func arguments() throws -> Arguments {
    var result = Arguments()
    var index = 1
    let values = CommandLine.arguments
    while index < values.count {
        switch values[index] {
        case "--list":
            result.listOnly = true
        case "--registry-id":
            index += 1
            guard index < values.count, let value = UInt64(values[index]) else {
                throw NSError(domain: "probe", code: 1, userInfo: [NSLocalizedDescriptionKey: "--registry-id needs a decimal IORegistry entry ID"])
            }
            result.registryID = value
        default:
            throw NSError(domain: "probe", code: 1, userInfo: [NSLocalizedDescriptionKey: "Unknown argument: \(values[index])"])
        }
        index += 1
    }
    guard result.listOnly != (result.registryID != nil) else {
        throw NSError(domain: "probe", code: 1, userInfo: [NSLocalizedDescriptionKey: "Use exactly one of --list or --registry-id <decimal-id>"])
    }
    return result
}

func property(_ device: IOHIDDevice, _ key: String) -> Any? {
    IOHIDDeviceGetProperty(device, key as CFString)
}

func number(_ device: IOHIDDevice, _ key: String) -> Int? {
    (property(device, key) as? NSNumber)?.intValue
}

func string(_ device: IOHIDDevice, _ key: String) -> String? {
    property(device, key) as? String
}

func elementNumber(_ element: IOHIDElement, _ key: String) -> Int? {
    (IOHIDElementGetProperty(element, key as CFString) as? NSNumber)?.intValue
}

func identity(_ device: IOHIDDevice, _ id: UInt64) -> [String: Any] {
    [
        "registryEntryId": String(id),
        "vendorId": number(device, kIOHIDVendorIDKey),
        "productId": number(device, kIOHIDProductIDKey),
        "locationId": number(device, kIOHIDLocationIDKey),
        "usagePage": number(device, kIOHIDPrimaryUsagePageKey),
        "usage": number(device, kIOHIDPrimaryUsageKey),
        "transport": string(device, kIOHIDTransportKey),
        "product": string(device, kIOHIDProductKey)
    ].compactMapValues { $0 }
}

func elements(_ device: IOHIDDevice) -> [[String: Any]] {
    let raw = IOHIDDeviceCopyMatchingElements(device, nil, IOOptionBits(kIOHIDOptionsTypeNone)) as? [IOHIDElement] ?? []
    return raw.sorted { IOHIDElementGetCookie($0) < IOHIDElementGetCookie($1) }.map { element in
        let parent = IOHIDElementGetParent(element)
        let parentCookie: Any = parent.map { Int(IOHIDElementGetCookie($0)) } ?? NSNull()
        return [
            "cookie": Int(IOHIDElementGetCookie(element)),
            "parentCookie": parentCookie,
            "type": Int(IOHIDElementGetType(element).rawValue),
            "collectionType": IOHIDElementGetType(element) == kIOHIDElementTypeCollection ? Int(IOHIDElementGetCollectionType(element).rawValue) as Any : NSNull(),
            "usagePage": Int(IOHIDElementGetUsagePage(element)),
            "usage": Int(IOHIDElementGetUsage(element)),
            "usageMin": elementNumber(element, kIOHIDElementUsageMinKey) as Any? ?? NSNull(),
            "usageMax": elementNumber(element, kIOHIDElementUsageMaxKey) as Any? ?? NSNull(),
            "reportId": Int(IOHIDElementGetReportID(element)),
            "reportSizeBits": Int(IOHIDElementGetReportSize(element)),
            "reportCount": Int(IOHIDElementGetReportCount(element)),
            "logicalMin": String(IOHIDElementGetLogicalMin(element)),
            "logicalMax": String(IOHIDElementGetLogicalMax(element)),
            "physicalMin": String(IOHIDElementGetPhysicalMin(element)),
            "physicalMax": String(IOHIDElementGetPhysicalMax(element)),
            "unit": Int(IOHIDElementGetUnit(element)),
            "unitExponent": Int(IOHIDElementGetUnitExponent(element)),
            "relative": IOHIDElementIsRelative(element),
            "wrapping": IOHIDElementIsWrapping(element),
            "nonLinear": IOHIDElementIsNonLinear(element),
            "preferredState": IOHIDElementHasPreferredState(element),
            "virtual": IOHIDElementIsVirtual(element),
            "array": IOHIDElementIsArray(element),
            "nullState": IOHIDElementHasNullState(element)
        ]
    }
}

func output(_ value: [String: Any]) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0a]))
}

do {
    let args = try arguments()
    let matching = IOServiceMatching("IOHIDDevice")
    guard matching != nil else { throw NSError(domain: "probe", code: 2, userInfo: [NSLocalizedDescriptionKey: "IOHIDDevice service matching is unavailable"]) }
    var iterator: io_iterator_t = IO_OBJECT_NULL
    guard IOServiceGetMatchingServices(kIOMasterPortDefault, matching, &iterator) == KERN_SUCCESS else {
        throw NSError(domain: "probe", code: 2, userInfo: [NSLocalizedDescriptionKey: "Cannot enumerate IOHIDDevice registry services"])
    }
    defer { IOObjectRelease(iterator) }
    var devices: [(UInt64, IOHIDDevice)] = []
    while true {
        let service = IOIteratorNext(iterator)
        if service == IO_OBJECT_NULL { break }
        var id: UInt64 = 0
        if IORegistryEntryGetRegistryEntryID(service, &id) == KERN_SUCCESS,
           let device = IOHIDDeviceCreate(kCFAllocatorDefault, service) {
            devices.append((id, device))
        }
        IOObjectRelease(service)
    }
    devices.sort { $0.0 < $1.0 }

    if args.listOnly {
        try output(["origin": "independent-macos-iohid-elements", "fixtureEvidence": false,
            "deviceOpened": false, "inputReads": false, "featureReads": false,
            "reportWrites": false, "devices": devices.map { identity($0.1, $0.0) }])
    } else {
        let matches = devices.filter { $0.0 == args.registryID }
        guard matches.count == 1, let (_, device) = matches.first else {
            throw NSError(domain: "probe", code: 3, userInfo: [NSLocalizedDescriptionKey: "The exact IORegistry entry ID must resolve to one current device"])
        }
        let descriptor = property(device, kIOHIDReportDescriptorKey) as? Data
        if let descriptor, descriptor.count > 64 * 1024 {
            throw NSError(domain: "probe", code: 4, userInfo: [NSLocalizedDescriptionKey: "Original report descriptor exceeds 64 KiB"])
        }
        let deviceElements = elements(device)
        guard !deviceElements.isEmpty, deviceElements.count <= 16_384 else {
            throw NSError(domain: "probe", code: 5, userInfo: [NSLocalizedDescriptionKey: "IOHID element count is empty or exceeds 16,384"])
        }
        var receipt: [String: Any] = ["origin": "independent-macos-iohid-elements", "fixtureEvidence": false,
            "deviceOpened": false, "inputReads": false, "featureReads": false,
            "reportWrites": false, "descriptorProvenance": "IOHIDDeviceGetProperty(ReportDescriptor)",
            "elementsProvenance": "IOHIDDeviceCopyMatchingElements", "device": identity(device, args.registryID!),
            "descriptorAvailable": false, "descriptorHex": NSNull(), "descriptorSha256": NSNull(),
            "elements": deviceElements]
        if let descriptor, !descriptor.isEmpty {
            receipt["descriptorAvailable"] = true
            receipt["descriptorHex"] = descriptor.map { String(format: "%02x", $0) }.joined()
            receipt["descriptorSha256"] = SHA256.hash(data: descriptor).map { String(format: "%02x", $0) }.joined()
        }
        try output(receipt)
    }
} catch {
    FileHandle.standardError.write(Data("\(error.localizedDescription)\n".utf8))
    exit(EXIT_FAILURE)
}
