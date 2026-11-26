import XCTest

/// The screens in light, dark and the largest Dynamic Type sizes (#521): each is audited by Apple's accessibility audit
/// and kept as a screenshot in the result bundle (no reference images: Linux cannot record them; the audit fails on
/// clipped text and low contrast).
final class AppearanceTests: XCTestCase {
    private let configurations: [(name: String, args: [String])] = [
        ("light", ["-AppleInterfaceStyle", "Light"]),
        ("dark", ["-AppleInterfaceStyle", "Dark"]),
        ("accessibility-xxxl", ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"]),
    ]

    func testSignInAndListsInEveryAppearance() throws {
        for configuration in configurations {
            let app = XCUIApplication()
            app.launchArguments += ["-reset"] + configuration.args
            app.launchEnvironment["MORPHEUS_HOMESERVER"] = "http://localhost:8008"
            app.launchEnvironment["MORPHEUS_FILE_SESSION_STORE"] = "1"
            app.launch()
            let user = app.textFields["sign-in-username"]
            XCTAssertTrue(user.waitForExistence(timeout: 20), configuration.name)
            keep(app, "sign-in-\(configuration.name)")
            try app.performAccessibilityAudit()

            user.tap()
            user.typeText("alice")
            app.secureTextFields["sign-in-password"].tap()
            app.secureTextFields["sign-in-password"].typeText("alice-pass-1")
            app.buttons["sign-in-submit"].tap()
            XCTAssertTrue(app.buttons["room-bob"].waitForExistence(timeout: 60), configuration.name)
            keep(app, "lists-\(configuration.name)")
            try app.performAccessibilityAudit()
            app.terminate()
        }
    }

    private func keep(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
