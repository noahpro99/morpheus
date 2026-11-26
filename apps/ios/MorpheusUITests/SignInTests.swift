import XCTest

final class SignInTests: MorpheusTestCase {
    func testSignInScreenIsAccessible() throws {
        XCTAssertTrue(app.textFields["sign-in-username"].waitForExistence(timeout: 20))
        try audit()
    }

    func testPasswordSignInShowsTheLists() throws {
        signIn()
        // Direct messages is the first list; bob's DM is in it.
        XCTAssertTrue(app.buttons["room-bob"].waitForExistence(timeout: 60), "the DM with bob")
        try audit()
    }

    func testWrongPasswordShowsAnError() throws {
        let user = app.textFields["sign-in-username"]
        XCTAssertTrue(user.waitForExistence(timeout: 20))
        user.tap()
        user.typeText("alice")
        app.secureTextFields["sign-in-password"].tap()
        app.secureTextFields["sign-in-password"].typeText("wrong")
        app.buttons["sign-in-submit"].tap()
        XCTAssertTrue(app.staticTexts["sign-in-error"].waitForExistence(timeout: 30))
    }
}
