import XCTest

/// The base of every test: a fresh app against the dev homeserver the workflow started (apps/ios/ci/homeserver.py),
/// and the Apple accessibility audit on every screen a test looks at.
class MorpheusTestCase: XCTestCase {
    var app = XCUIApplication()

    override func setUpWithError() throws {
        continueAfterFailure = false
        app.launchArguments += ["-reset"]
        app.launchEnvironment["MORPHEUS_HOMESERVER"] = "http://localhost:8008"
        app.launchEnvironment["MORPHEUS_FILE_SESSION_STORE"] = "1"
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
        XCTAssertTrue(app.buttons["list-picker"].exists || app.segmentedControls["list-picker"].waitForExistence(timeout: 60), "the lists after signing in")
    }

    /// Apple's audit: contrast, hit regions, Dynamic Type, labels, clipped text.
    func audit(file: StaticString = #filePath, line: UInt = #line) throws {
        try app.auditing()
    }
}

extension MorpheusTestCase {
    /// What the screen had when a test failed: the accessibility tree, for the CI log.
    override func tearDown() {
        if (testRun?.failureCount ?? 0) > 0 {
            print("--- accessibility tree at failure ---\n\(app.debugDescription)")
        }
        super.tearDown()
    }
}

extension XCUIApplication {
    /// Apple's audit, with every issue (type, what, which element) printed for the CI log; any issue fails.
    func auditing() throws {
        try performAccessibilityAudit { issue in
            print("AUDIT \(issue.auditType.rawValue) \(issue.compactDescription) | \(issue.element?.debugDescription.prefix(160) ?? "no element")")
            return false
        }
    }
}
