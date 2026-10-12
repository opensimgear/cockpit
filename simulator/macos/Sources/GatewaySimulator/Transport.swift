import Darwin
import Foundation

enum Transport {
    static func connectSocket(_ path: String) throws -> Int32 {
        guard path.hasPrefix("/"), path.utf8.count < 104 else { throw SimulatorError.invalid("Unix socket path must be absolute and shorter than 104 bytes.") }
        var info = stat()
        guard lstat(path, &info) == 0, (info.st_mode & S_IFMT) == S_IFSOCK else { throw SimulatorError.invalid("The Cockpit bridge socket does not exist.") }
        let directory = (path as NSString).deletingLastPathComponent
        var directoryInfo = stat()
        guard lstat(directory, &directoryInfo) == 0, (directoryInfo.st_mode & S_IFMT) == S_IFDIR,
              directoryInfo.st_uid == info.st_uid, directoryInfo.st_mode & 0o077 == 0 else {
            throw SimulatorError.invalid("The Cockpit bridge socket must be inside a private directory owned by its user.")
        }
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw POSIXError(.ENOTSOCK) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        withUnsafeMutableBytes(of: &address.sun_path) { bytes in path.utf8CString.withUnsafeBytes { bytes.copyBytes(from: $0) } }
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard connected == 0 else { close(fd); throw SimulatorError.invalid("Cannot connect to Cockpit's local bridge: \(String(cString: strerror(errno))).") }
        var peerUID: uid_t = 0; var peerGID: gid_t = 0
        guard getpeereid(fd, &peerUID, &peerGID) == 0, peerUID == info.st_uid else { close(fd); throw SimulatorError.invalid("Cockpit bridge peer ownership does not match the socket.") }
        return fd
    }

    static func lines(fd: Int32) -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream { continuation in
            Task.detached {
                var pending = Data()
                var bytes = [UInt8](repeating: 0, count: 4096)
                while true {
                    let count = Darwin.read(fd, &bytes, bytes.count)
                    if count == 0 { break }
                    if count < 0 {
                        if errno == EINTR { continue }
                        continuation.finish(throwing: POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)); return
                    }
                    pending.append(contentsOf: bytes.prefix(count))
                    while let newline = pending.firstIndex(of: 10) {
                        let line = Data(pending[..<newline]); pending.removeSubrange(...newline)
                        guard line.count <= 65536 else { continuation.finish(throwing: SimulatorError.invalid("Simulator request exceeds 64 KiB.")); return }
                        if !line.isEmpty { continuation.yield(line) }
                    }
                    guard pending.count <= 65536 else { continuation.finish(throwing: SimulatorError.invalid("Simulator request exceeds 64 KiB.")); return }
                }
                if !pending.isEmpty { continuation.yield(pending) }
                continuation.finish()
            }
        }
    }
}

actor JSONWriter {
    private let fd: Int32
    private let encoder = JSONEncoder()
    init(fd: Int32) { self.fd = fd; encoder.outputFormatting = [.sortedKeys] }
    func write(_ response: Response) {
        guard var data = try? encoder.encode(response) else { return }
        data.append(10)
        data.withUnsafeBytes { buffer in
            guard let base = buffer.baseAddress else { return }
            var sent = 0
            while sent < buffer.count {
                let count = Darwin.write(fd, base.advanced(by: sent), buffer.count - sent)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { return }
                sent += count
            }
        }
    }
}
