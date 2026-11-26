import XCTest

final class ListsTests: MorpheusTestCase {
    func testSwipingMovesBetweenDirectMessagesAndServers() throws {
        signIn()
        XCTAssertTrue(app.buttons["room-bob"].waitForExistence(timeout: 60))
        // No bottom tab bar.
        XCTAssertEqual(app.tabBars.count, 0)
        app.swipeLeft()
        XCTAssertTrue(app.buttons["server-Test Server"].waitForExistence(timeout: 10), "the servers list")
        try audit()
        app.swipeRight()
        XCTAssertTrue(app.buttons["room-bob"].waitForExistence(timeout: 10))
    }

    func testListRowsRestAtTheBottom() throws {
        signIn()
        let row = app.buttons["room-bob"]
        XCTAssertTrue(row.waitForExistence(timeout: 60))
        // The only row sits in the lower half of the screen, not at the top.
        XCTAssertGreaterThan(row.frame.midY, app.windows.firstMatch.frame.height / 2)
    }

    func testServerOpensItsChannelsAndEdgeSwipeGoesBack() throws {
        signIn()
        _ = app.buttons["room-bob"].waitForExistence(timeout: 60)
        app.swipeLeft()
        app.buttons["server-Test Server"].tap()
        XCTAssertTrue(app.buttons["room-general"].waitForExistence(timeout: 10) || app.staticTexts["general"].waitForExistence(timeout: 10))
        // The system's edge swipe.
        let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.0, dy: 0.5))
        start.press(forDuration: 0.1, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5)))
        XCTAssertTrue(app.buttons["server-Test Server"].waitForExistence(timeout: 10), "back on the servers list")
    }
}
