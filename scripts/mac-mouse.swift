// The mouse as a person uses it, for scripts/mac-check.sh (System Events
// clicks but does not drag): global screen points, top-left origin, as
// System Events reports a window's position.
//
//   swift scripts/mac-mouse.swift drag X Y DX DY
//   swift scripts/mac-mouse.swift doubleclick X Y
import CoreGraphics
import Foundation

let args = CommandLine.arguments
func number(_ i: Int) -> Double {
    guard i < args.count, let n = Double(args[i]) else {
        FileHandle.standardError.write("usage: mac-mouse.swift drag X Y DX DY | doubleclick X Y\n".data(using: .utf8)!)
        exit(2)
    }
    return n
}

// Each event carries its movement from the last, as a real mouse's do (a
// drag by the system's title bar may move the window by those, not by the
// positions).
//
// From the HID system state, as a real mouse's events are (MAC_MOUSE_SOURCE=none
// posts them with no source, as before #68).
let source = ProcessInfo.processInfo.environment["MAC_MOUSE_SOURCE"] == "none"
    ? nil : CGEventSource(stateID: .hidSystemState)
var last: CGPoint? = nil
func post(_ type: CGEventType, _ at: CGPoint, clicks: Int64 = 1) {
    guard let event = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: at, mouseButton: .left) else {
        FileHandle.standardError.write("could not make a mouse event\n".data(using: .utf8)!)
        exit(1)
    }
    event.setIntegerValueField(.mouseEventClickState, value: clicks)
    if let from = last {
        event.setIntegerValueField(.mouseEventDeltaX, value: Int64((at.x - from.x).rounded()))
        event.setIntegerValueField(.mouseEventDeltaY, value: Int64((at.y - from.y).rounded()))
    }
    last = at
    event.post(tap: .cghidEventTap)
    usleep(30_000)
}

let start = CGPoint(x: number(2), y: number(3))
switch args.count > 1 ? args[1] : "" {
case "drag":
    let (dx, dy) = (number(4), number(5))
    post(.mouseMoved, start)
    usleep(300_000)
    post(.leftMouseDown, start)
    usleep(300_000)
    for i in 1...20 {
        let f = Double(i) / 20
        post(.leftMouseDragged, CGPoint(x: start.x + dx * f, y: start.y + dy * f))
    }
    usleep(300_000)
    post(.leftMouseUp, CGPoint(x: start.x + dx, y: start.y + dy))
case "doubleclick":
    post(.mouseMoved, start)
    usleep(300_000)
    post(.leftMouseDown, start, clicks: 1)
    post(.leftMouseUp, start, clicks: 1)
    post(.leftMouseDown, start, clicks: 2)
    post(.leftMouseUp, start, clicks: 2)
default:
    _ = number(99)
}
usleep(500_000)
if let at = CGEvent(source: nil)?.location {
    print("mouse at \(Int(at.x)),\(Int(at.y))")
}
