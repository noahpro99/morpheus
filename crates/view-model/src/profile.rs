//! The profile card and full profile (`crates/ui/src/components/profile.rs`):
//! where a card sits beside what opened it, how it counts the servers you
//! share, and the role label under the name. The front end fetches the
//! person and draws the card.

use morpheus_core::{MemberRole, OwnedRoomId, OwnedUserId, RoomSummary};

use super::menu::{MenuAction, MenuItem};

/// The card's width.
pub const CARD_W: f32 = 340.0;
/// The gap kept to the app's edges, and beside the click.
const PAD: f32 = 8.0;
const BESIDE: f32 = 16.0;

/// A rectangle in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where the card's top left goes: beside the click on the side nearer the
/// middle of the app (never over it), and always inside the app. With no
/// click it is centred, a fifth of the way down.
pub fn place_card(click: Option<(f32, f32)>, size: (f32, f32), app: Area) -> (f32, f32) {
    let (w, h) = size;
    let (left, top) = (app.x, app.y);
    let (right, bottom) = (app.x + app.w, app.y + app.h);
    let (x, y) = match click {
        Some((cx, cy)) => (
            if cx < (left + right) / 2.0 {
                cx + BESIDE
            } else {
                cx - w - BESIDE
            },
            cy,
        ),
        None => ((left + right - w) / 2.0, top + (bottom - top) * 0.2),
    };
    let y = y.min(bottom - h - PAD).max(top + PAD);
    let x = x.min(right - w - PAD).max(left + PAD);
    (x, y)
}

/// The card's tallest height: the app's, less its padding (it scrolls inside).
pub fn card_max_height(app: Area) -> f32 {
    app.h - 2.0 * PAD
}

/// "1 mutual server", "3 mutual servers".
pub fn mutual_servers_label(count: usize) -> String {
    if count == 1 {
        "1 mutual server".to_owned()
    } else {
        format!("{count} mutual servers")
    }
}

/// The heading of the full profile's list: "Mutual servers — 2".
pub fn mutual_heading(count: usize) -> String {
    format!("Mutual servers — {count}")
}

/// The role tag: what they are in the open room.
pub fn role_tag(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Admin => "Admin",
        MemberRole::Moderator => "Moderator",
        MemberRole::Member => "Member",
    }
}

/// The servers (top-level spaces) among `spaces`, by name.
pub fn shared_servers(rooms: &[RoomSummary], spaces: &[OwnedRoomId]) -> Vec<RoomSummary> {
    let mut servers: Vec<RoomSummary> = rooms
        .iter()
        .filter(|r| r.is_space && r.parent_spaces.is_empty() && spaces.contains(&r.id))
        .cloned()
        .collect();
    servers.sort_by_key(|s| s.name.to_lowercase());
    servers
}

/// The card's "…" menu: the full profile, share, invite, block, report
/// and copying (`invite_to`: where they can be invited, id and name).
pub fn more_menu(
    user: &OwnedUserId,
    own: bool,
    blocked: bool,
    invite_to: &[(OwnedRoomId, String)],
) -> Vec<MenuItem> {
    let link = format!("https://matrix.to/#/{user}");
    let mut items = vec![
        MenuItem::new("View full profile", MenuAction::FullProfile(user.clone())).icon("user"),
        MenuItem::new("Share profile", MenuAction::ShareProfile(link.clone())).icon("share-2"),
    ];
    if !own {
        if !invite_to.is_empty() {
            let children = invite_to
                .iter()
                .map(|(id, name)| {
                    MenuItem::new(name.clone(), MenuAction::Invite(id.clone(), user.clone()))
                        .avatar(name.clone())
                })
                .collect();
            items.push(MenuItem::submenu("Invite to server", children).icon("user-plus"));
        }
        items.push(
            MenuItem::new(
                if blocked { "Unblock" } else { "Block" },
                MenuAction::Block(user.clone(), !blocked),
            )
            .icon("ban"),
        );
        items.push(
            MenuItem::new("Report user profile", MenuAction::ReportUser(user.clone())).icon("flag"),
        );
    }
    items.push(
        MenuItem::submenu(
            "Copy user info",
            vec![
                MenuItem::new("Copy user ID", MenuAction::CopyId(user.to_string())).icon("hash"),
                MenuItem::new("Copy profile link", MenuAction::CopyLink(link)).icon("link"),
            ],
        )
        .icon("copy"),
    );
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_room::room;

    fn bob() -> OwnedUserId {
        OwnedUserId::try_from("@bob:x").expect("user id")
    }

    #[test]
    fn the_more_menu_lists_what_the_card_offers() {
        let invite = [(OwnedRoomId::try_from("!s:x").expect("id"), "Engineering".to_owned())];
        let items = more_menu(&bob(), false, false, &invite);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "View full profile",
                "Share profile",
                "Invite to server",
                "Block",
                "Report user profile",
                "Copy user info"
            ]
        );
        let copy = items.last().expect("copy");
        assert_eq!(
            copy.children.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["Copy user ID", "Copy profile link"]
        );
        assert_eq!(
            copy.children[1].action,
            Some(MenuAction::CopyLink("https://matrix.to/#/@bob:x".into()))
        );
    }

    #[test]
    fn your_own_card_has_no_block_report_or_invite_and_a_blocked_one_offers_unblock() {
        let own = more_menu(&bob(), true, false, &[]);
        let labels: Vec<_> = own.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["View full profile", "Share profile", "Copy user info"]);
        let blocked = more_menu(&bob(), false, true, &[]);
        let unblock = blocked.iter().find(|i| i.label == "Unblock").expect("unblock");
        assert_eq!(unblock.action, Some(MenuAction::Block(bob(), false)));
        assert!(blocked.iter().all(|i| i.glyph().is_some()), "every row has an icon");
    }

    const APP: Area = Area {
        x: 0.0,
        y: 0.0,
        w: 1000.0,
        h: 700.0,
    };

    #[test]
    fn the_card_sits_beside_the_click_on_the_side_nearer_the_middle() {
        // A click on the left half: the card goes to its right.
        let (x, y) = place_card(Some((100.0, 200.0)), (340.0, 300.0), APP);
        assert_eq!((x, y), (116.0, 200.0));
        // On the right half: to its left, never over it.
        let (x, _) = place_card(Some((900.0, 200.0)), (340.0, 300.0), APP);
        assert_eq!(x, 900.0 - 340.0 - 16.0);
    }

    #[test]
    fn the_card_stays_inside_the_app() {
        // Low down: pushed up so its bottom edge keeps the padding.
        let (_, y) = place_card(Some((100.0, 690.0)), (340.0, 300.0), APP);
        assert_eq!(y, 700.0 - 300.0 - 8.0);
        // Near the top: pushed down to the padding.
        let (_, y) = place_card(Some((100.0, 0.0)), (340.0, 300.0), APP);
        assert_eq!(y, 8.0);
        // Far right and a wide card: held by the right edge.
        let (x, _) = place_card(Some((490.0, 100.0)), (340.0, 300.0), APP);
        assert!(x + 340.0 <= 1000.0 - 8.0);
        // A card taller than the app starts at the top padding.
        let (_, y) = place_card(Some((100.0, 300.0)), (340.0, 900.0), APP);
        assert_eq!(y, 8.0);
    }

    #[test]
    fn without_a_click_it_is_centred_a_fifth_of_the_way_down() {
        let (x, y) = place_card(None, (340.0, 300.0), APP);
        assert_eq!(x, 330.0);
        assert_eq!(y, 140.0);
    }

    #[test]
    fn it_scrolls_inside_when_the_app_is_short() {
        assert_eq!(card_max_height(APP), 684.0);
    }

    #[test]
    fn labels() {
        assert_eq!(mutual_servers_label(0), "0 mutual servers");
        assert_eq!(mutual_servers_label(1), "1 mutual server");
        assert_eq!(mutual_servers_label(3), "3 mutual servers");
        assert_eq!(mutual_heading(2), "Mutual servers — 2");
        assert_eq!(role_tag(MemberRole::Moderator), "Moderator");
    }

    #[test]
    fn only_top_level_servers_you_share_count_sorted_by_name() {
        let (mut b, mut a, mut nested, other) = (
            room("!b:x", "beta"),
            room("!a:x", "Alpha"),
            room("!n:x", "nested"),
            room("!o:x", "other"),
        );
        b.is_space = true;
        a.is_space = true;
        nested.is_space = true;
        nested.parent_spaces = vec![a.id.clone()];
        let rooms = [b.clone(), a.clone(), nested.clone(), other.clone()];
        let spaces = [b.id.clone(), a.id.clone(), nested.id.clone(), other.id.clone()];
        let names: Vec<_> = shared_servers(&rooms, &spaces)
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["Alpha", "beta"]);
        assert!(shared_servers(&rooms, &[]).is_empty());
    }
}
