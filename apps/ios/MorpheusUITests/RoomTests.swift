import XCTest

final class RoomTests: MorpheusTestCase {
    private func openGeneral() {
        signIn()
        _ = app.buttons["room-bob"].waitForExistence(timeout: 60)
        app.swipeLeft()
        app.buttons["server-Test Server"].tap()
        let general = app.buttons["room-general"].firstMatch
        XCTAssertTrue(general.waitForExistence(timeout: 20), "the general channel")
        general.tap()
    }

    func testRoomShowsTheNewestMessagesAndLoadsOlderHistory() throws {
        openGeneral()
        XCTAssertTrue(app.staticTexts["history 60"].waitForExistence(timeout: 30), "the newest message")
        try audit()
        // Pull the history down until the first message is there.
        let first = app.staticTexts["history 1"]
        for _ in 0..<30 where !first.exists {
            app.scrollViews["timeline"].swipeDown()
        }
        XCTAssertTrue(first.waitForExistence(timeout: 30), "older history loaded")
    }

    func testSendShowsAGreyedLocalEchoAtOnce() throws {
        openGeneral()
        XCTAssertTrue(app.textFields["composer"].waitForExistence(timeout: 30))
        app.textFields["composer"].tap()
        app.textFields["composer"].typeText("ping from ios")
        app.buttons["send"].tap()
        // The echo is on screen within a second, before the server round trip (it is "sending" until then).
        XCTAssertTrue(app.staticTexts["ping from ios"].waitForExistence(timeout: 1.5), "local echo")
    }

    func testEdgeSwipeLeavesTheRoom() throws {
        openGeneral()
        XCTAssertTrue(app.textFields["composer"].waitForExistence(timeout: 30))
        let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.0, dy: 0.5))
        start.press(forDuration: 0.1, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5)))
        XCTAssertFalse(app.textFields["composer"].waitForExistence(timeout: 3))
    }
}
