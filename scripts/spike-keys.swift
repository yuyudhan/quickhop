// Types text into one process with real key events, for the gpui toolkit spike
// (crates/quickhop-ui/examples/spike.rs, plan slice U0). Events go to that process only
// (CGEventPostToPid), never to the frontmost app. Needs Accessibility permission.
//
//     swift scripts/spike-keys.swift <pid> <text> [interval-ms]
//
// Letters a-z and spaces only (US ANSI key codes); `-` in <text> types Backspace.
import CoreGraphics
import Foundation

let keyCodes: [Character: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9,
    "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "o": 31, "u": 32,
    "i": 34, "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, " ": 49, "-": 51,
]

let arguments = CommandLine.arguments
guard arguments.count >= 3, let pid = pid_t(arguments[1]) else {
    FileHandle.standardError.write("usage: spike-keys.swift <pid> <text> [interval-ms]\n".data(using: .utf8)!)
    exit(2)
}
let interval = arguments.count > 3 ? (Double(arguments[3]) ?? 50) : 50
let source = CGEventSource(stateID: .hidSystemState)

for character in arguments[2] {
    guard let code = keyCodes[character] else {
        FileHandle.standardError.write("no key code for \(character)\n".data(using: .utf8)!)
        exit(2)
    }
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else {
            exit(1)
        }
        if character != "-" {
            var unit = Array(String(character).utf16)
            event.keyboardSetUnicodeString(stringLength: unit.count, unicodeString: &unit)
        }
        event.postToPid(pid)
    }
    usleep(useconds_t(interval * 1000))
}
