// swift-tools-version: 5.9
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "WavryMacOS",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "WavryMacOS", targets: ["WavryMacOS"]),
    ],
    targets: [
        .target(
            name: "Clibwavry",
            path: "Sources/Clibwavry"
            // Header search path is automatic for "include"
        ),
        .executableTarget(
            name: "WavryMacOS",
            dependencies: ["Clibwavry"],
            resources: [
                .process("Resources")
            ],
            linkerSettings: [
                .linkedFramework("VideoToolbox"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("CoreFoundation"),
                .unsafeFlags([
                    "-L../../target/debug",
                    "-L../../target/release",
                    "-lwavry_ffi"
                ])
            ]
        ),
    ]
)
