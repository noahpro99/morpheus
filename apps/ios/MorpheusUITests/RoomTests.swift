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
        XCTAssertTrue(app.staticTexts["history 60"].waitForExistence(timeout: 60), "the room has loaded")
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
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: app.textFields["composer"])
        XCTAssertEqual(XCTWaiter().wait(for: [gone], timeout: 10), .completed, "the room is left")
    }

    /// Send `text` and wait until the server has it (the row is no longer "sending").
    private func sendAndSettle(_ text: String) {
        app.textFields["composer"].tap()
        app.textFields["composer"].typeText(text)
        app.buttons["send"].tap()
        let sent = NSPredicate(format: "exists == true AND value == 'sent'")
        let row = app.staticTexts.matching(identifier: "message").matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
        XCTAssertEqual(XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: sent, object: row)], timeout: 60), .completed, "\(text) reached the server")
    }

    private func longPressMessage(containing text: String) {
        let row = app.staticTexts.matching(identifier: "message").matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), text)
        row.press(forDuration: 1.0)
    }

    func testReplyShowsTheQuoteAndReactionAppearsAsAChip() throws {
        openGeneral()
        XCTAssertTrue(app.staticTexts["history 60"].waitForExistence(timeout: 60))
        longPressMessage(containing: "history 60")
        app.buttons["Reply"].tap()
        XCTAssertTrue(app.staticTexts["composer-mode"].waitForExistence(timeout: 5), "the composer says it is a reply")
        app.textFields["composer"].typeText("answering")
        app.buttons["send"].tap()
        XCTAssertTrue(app.otherElements["reply-quote"].firstMatch.waitForExistence(timeout: 30) || app.staticTexts["reply-quote"].firstMatch.waitForExistence(timeout: 5), "the reply quotes its message")

        longPressMessage(containing: "history 59")
        app.buttons["\u{1F44D}"].tap()
        XCTAssertTrue(app.buttons["reaction"].firstMatch.waitForExistence(timeout: 30), "the reaction is a chip under the message")
        try audit()
    }

    func testEditingAndDeletingOwnMessage() throws {
        openGeneral()
        XCTAssertTrue(app.textFields["composer"].waitForExistence(timeout: 30))
        sendAndSettle("first draft")
        longPressMessage(containing: "first draft")
        app.buttons["Edit"].tap()
        let composer = app.textFields["composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 5))
        composer.typeText(" edited")
        app.buttons["send"].tap()
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'edited)'")).firstMatch.waitForExistence(timeout: 30), "marked as edited")

        longPressMessage(containing: "first draft")
        app.buttons["Delete"].tap()
        let row = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "first draft")).firstMatch
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: row)
        XCTAssertEqual(XCTWaiter().wait(for: [gone], timeout: 30), .completed, "deleted")
    }
}
