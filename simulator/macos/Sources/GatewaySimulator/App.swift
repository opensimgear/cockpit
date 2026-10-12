import Darwin
import Foundation

@main
struct SimulatorApp {
    static func main() async {
        signal(SIGPIPE, SIG_IGN)
        let arguments = Array(CommandLine.arguments.dropFirst())
        let fd: Int32
        do {
            if arguments.isEmpty { fd = STDIN_FILENO }
            else if arguments.count == 2 && arguments[0] == "--connect-socket" { fd = try Transport.connectSocket(arguments[1]) }
            else { throw SimulatorError.invalid("Usage: osg-gateway-simulator [--connect-socket /absolute/private/gateway.sock]") }
        } catch {
            FileHandle.standardError.write(Data("\(error.localizedDescription)\n".utf8)); exit(1)
        }
        let writer = JSONWriter(fd: fd == STDIN_FILENO ? STDOUT_FILENO : fd)
        let gateway = Gateway()
        var tasks: [Task<Void, Never>] = []
        do {
            for try await line in Transport.lines(fd: fd) {
                do {
                    let request = try JSONDecoder().decode(Request.self, from: line)
                    tasks.append(Task { await writer.write(await gateway.handle(request)) })
                    if tasks.count >= 64 { await tasks.removeFirst().value }
                } catch {
                    let object = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any]
                    let decodedID = object?["id"] as? String
                    let responseID = decodedID.map { !$0.isEmpty && $0.count <= 100 ? $0 : "invalid" } ?? "invalid"
                    await writer.write(Response(id: responseID, ok: false, state: await gateway.snapshot(), error: "Invalid simulator request JSON: \(error.localizedDescription)"))
                }
            }
        } catch { FileHandle.standardError.write(Data("\(error.localizedDescription)\n".utf8)) }
        for task in tasks { await task.value }
        await gateway.stop()
        if fd != STDIN_FILENO { close(fd) }
    }
}
