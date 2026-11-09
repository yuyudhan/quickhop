// The app a paste test pastes into (`quickhop_uitest::ax::PasteTarget`): one plain-text
// window that takes the focus and writes what it holds to the file named by its first
// argument on every change. It quits when asked to, or after 60 s.
import AppKit

final class Target: NSObject, NSApplicationDelegate, NSTextViewDelegate {
    let output: String
    var window: NSWindow?

    init(output: String) {
        self.output = output
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(
            contentRect: NSRect(x: 240, y: 240, width: 420, height: 160),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "quickhop paste target"
        let text = NSTextView(frame: window.contentLayoutRect)
        text.isRichText = false
        text.delegate = self
        window.contentView = text
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(text)
        self.window = window
        NSApp.activate(ignoringOtherApps: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 60) { NSApp.terminate(nil) }
    }

    func textDidChange(_ notification: Notification) {
        guard let text = notification.object as? NSTextView else { return }
        try? text.string.write(toFile: output, atomically: true, encoding: .utf8)
    }
}

// ⌘V reaches a text view through the Edit menu's Paste item.
let edit = NSMenu(title: "Edit")
edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
let editItem = NSMenuItem()
editItem.submenu = edit
let menu = NSMenu()
menu.addItem(NSMenuItem())
menu.addItem(editItem)

let app = NSApplication.shared
app.setActivationPolicy(.regular)
app.mainMenu = menu
let target = Target(output: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "/dev/null")
app.delegate = target
app.run()
