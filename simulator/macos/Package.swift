// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "OpenSimGearGatewaySimulator",
    platforms: [.macOS(.v15)],
    products: [.executable(name: "osg-gateway-simulator", targets: ["GatewaySimulator"])],
    targets: [
        .executableTarget(name: "GatewaySimulator"),
        .testTarget(name: "GatewaySimulatorTests", dependencies: ["GatewaySimulator"]),
    ]
)
