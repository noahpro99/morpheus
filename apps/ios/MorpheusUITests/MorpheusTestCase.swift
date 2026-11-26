import XCTest

/// The base of every test: a fresh app against the dev homeserver the workflow started (apps/ios/ci/homeserver.py),
/// and the Apple accessibility audit on every screen a test looks at.
class MorpheusTestCase: XCTestCase {
    var app = XCUIApplication()

    override func setUpWithError() throws {
        continueAfterFailure = false
        app.launchArguments += ["-reset"]
        app.launchEnvironment["MORPHEUS_HOMESERVER"] = "http://localhost:8008"
        app.launch()
    }

    /// Sign in as `alice` and wait for the lists.
    func signIn() {
        let user = app.textFields["sign-in-username"]
        XCTAssertTrue(user.waitForExistence(timeout: 20), "the sign-in screen")
        user.tap()
        user.typeText("alice")
        let password = app.secureTextFields["sign-in-password"]
        password.tap()
        password.typeText("alice-pass-1")
        app.buttons["sign-in-submit"].tap()
        XCTAssertTrue(app.otherElements["lists"].waitForExistence(timeout: 60) || app.scrollViews.firstMatch.waitForExistence(timeout: 60), "the lists after signing in")
    }

    /// Apple's audit: contrast, hit regions, Dynamic Type, labels, clipped text.
    func audit(file: StaticString = #filePath, line: UInt = #line) throws {
        try app.performAccessibilityAudit()
    }
}
