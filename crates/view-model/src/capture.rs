//! Whether another program is capturing the screen (#143): on Linux, from
//! PipeWire's own dump (`pw-dump`): a screen source (the portal's node, a
//! `Video/Source` named for the portal with no role, or an older
//! `Stream/Output/Video`; never a camera) linked, actively, to a consumer
//! that is not this program (a Discord stream, OBS). The consumer's process
//! is on its client object, not on the node. Pure: the app asks the system
//! and hands the text here.

use serde_json::Value;

/// Whether `pw_dump` (PipeWire's JSON dump of its graph) has an active
/// screen link to a program other than the one with process id `our_pid`.
pub fn external_capture(pw_dump: &str, our_pid: u32) -> bool {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(pw_dump) else {
        return false;
    };
    let find = |kind: &str, id: u64| {
        objects
            .iter()
            .find(|o| o["type"] == kind && o["id"].as_u64() == Some(id))
    };
    let node = |id: u64| find("PipeWire:Interface:Node", id);
    let text = |p: &Value, key: &str| p[key].as_str().unwrap_or_default().to_owned();
    let number = |p: &Value, key: &str| {
        p[key]
            .as_u64()
            .or_else(|| p[key].as_str().and_then(|s| s.parse().ok()))
    };
    // A screen, not a camera (a camera has the role Camera, or is a v4l2 device).
    let is_screen = |p: &Value| {
        let (class, role, name) = (
            text(p, "media.class"),
            text(p, "media.role"),
            text(p, "node.name"),
        );
        matches!(class.as_str(), "Stream/Output/Video" | "Video/Source")
            && role != "Camera"
            && !name.starts_with("v4l2")
    };
    // Whether the consumer is this program: its client's pids (the one it
    // says and the one PipeWire saw can differ), or its own, if it has one.
    let is_ours = |reader: &Value| {
        let ours = Some(u64::from(our_pid));
        if number(reader, "application.process.id") == ours {
            return true;
        }
        number(reader, "client.id")
            .and_then(|id| find("PipeWire:Interface:Client", id))
            .is_some_and(|client| {
                let p = &client["info"]["props"];
                number(p, "application.process.id") == ours
                    || number(p, "pipewire.sec.pid") == ours
            })
    };
    objects.iter().any(|link| {
        if link["type"] != "PipeWire:Interface:Link" || link["info"]["state"] != "active" {
            return false;
        }
        let (Some(from), Some(to)) = (
            link["info"]["output-node-id"].as_u64().and_then(node),
            link["info"]["input-node-id"].as_u64().and_then(node),
        ) else {
            return false;
        };
        let (source, reader) = (&from["info"]["props"], &to["info"]["props"]);
        is_screen(source) && !is_ours(reader)
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    const OURS: u32 = 4242;

    /// A node as `pw-dump` shows one: its class, name, role and the client
    /// that owns it (a stream node carries no process id of its own).
    fn node(id: u32, class: &str, name: &str, role: Option<&str>, client: u32) -> String {
        let role = role.map_or(String::new(), |r| format!(r#""media.role": "{r}","#));
        format!(
            r#"{{"id": {id}, "type": "PipeWire:Interface:Node", "info": {{"props": {{{role}"client.id": {client}, "media.class": "{class}", "node.name": "{name}"}}}}}}"#
        )
    }

    /// A client object: the pid it says it is, and the one PipeWire saw.
    fn client(id: u32, app_pid: u32, sec_pid: u32) -> String {
        format!(
            r#"{{"id": {id}, "type": "PipeWire:Interface:Client", "info": {{"props": {{"application.process.id": {app_pid}, "pipewire.sec.pid": {sec_pid}, "application.name": "electron"}}}}}}"#
        )
    }

    fn link(id: u32, from: u32, to: u32, state: &str) -> String {
        format!(
            r#"{{"id": {id}, "type": "PipeWire:Interface:Link", "info": {{"output-node-id": {from}, "input-node-id": {to}, "state": "{state}"}}}}"#
        )
    }

    fn dump(parts: &[String]) -> String {
        format!("[{}]", parts.join(","))
    }

    const PORTAL: &str = ".xdg-desktop-portal-hyprland-wrapped";

    /// What a Discord stream on Hyprland looked like: the portal's screen as a
    /// `Video/Source` with no role, read by `electron`'s `Stream/Input/Video`,
    /// whose pids are on its client (two different ones).
    fn discord_stream() -> Vec<String> {
        vec![
            client(59, 900, 900),
            node(145, "Video/Source", PORTAL, None, 59),
            client(180, 2271253, 2271438),
            node(181, "Stream/Input/Video", "electron", None, 180),
            link(182, 145, 181, "active"),
        ]
    }

    #[test]
    fn nothing_linked_is_no_capture() {
        let d = dump(&[node(145, "Video/Source", PORTAL, None, 59)]);
        assert!(!external_capture(&d, OURS));
        assert!(!external_capture("[]", OURS));
        assert!(!external_capture("not json", OURS));
    }

    #[test]
    fn a_discord_stream_on_the_portals_video_source_is_a_capture() {
        assert!(external_capture(&dump(&discord_stream()), OURS));
    }

    #[test]
    fn the_older_screen_cast_stream_is_still_a_capture() {
        let d = dump(&[
            client(2, 111, 111),
            node(1, "Stream/Output/Video", "xdg-desktop-portal", Some("Screen"), 59),
            node(3, "Stream/Input/Video", "obs", None, 2),
            link(4, 1, 3, "active"),
        ]);
        assert!(external_capture(&d, OURS));
    }

    #[test]
    fn our_own_capture_is_not_external_by_either_pid() {
        for (app, sec) in [(OURS, 7), (7, OURS), (OURS, OURS)] {
            let d = dump(&[
                node(145, "Video/Source", PORTAL, None, 59),
                client(180, app, sec),
                node(181, "Stream/Input/Video", "morpheus-desktop", None, 180),
                link(182, 145, 181, "active"),
            ]);
            assert!(!external_capture(&d, OURS), "pids {app} and {sec}");
        }
    }

    #[test]
    fn a_camera_is_not_a_screen_capture() {
        let by_role = dump(&[
            node(57, "Video/Source", "v4l2_input.pci-0000_00_14.0-usb-0_3_1.0", Some("Camera"), 35),
            client(180, 111, 111),
            node(181, "Stream/Input/Video", "electron", None, 180),
            link(182, 57, 181, "active"),
        ]);
        assert!(!external_capture(&by_role, OURS), "a webcam in a call");
        let by_name = dump(&[
            node(57, "Video/Source", "v4l2_input.usb-0_3_1.2", None, 35),
            client(180, 111, 111),
            node(181, "Stream/Input/Video", "electron", None, 180),
            link(182, 57, 181, "active"),
        ]);
        assert!(!external_capture(&by_name, OURS), "a camera device node");
        let stream = dump(&[
            node(1, "Stream/Output/Video", "xdg-desktop-portal", Some("Camera"), 59),
            client(180, 111, 111),
            node(181, "Stream/Input/Video", "electron", None, 180),
            link(182, 1, 181, "active"),
        ]);
        assert!(!external_capture(&stream, OURS), "a camera stream");
    }

    #[test]
    fn a_paused_or_unlinked_stream_is_not_a_capture() {
        let mut parts = discord_stream();
        parts.pop();
        parts.push(link(182, 145, 181, "paused"));
        assert!(!external_capture(&dump(&parts), OURS));
    }

    #[test]
    fn ours_and_theirs_together_is_external() {
        let mut parts = discord_stream();
        parts.extend([
            node(116, "Video/Source", PORTAL, None, 59),
            client(124, OURS, OURS),
            node(102, "Stream/Input/Video", "morpheus-desktop", None, 124),
            link(103, 116, 102, "active"),
        ]);
        assert!(external_capture(&dump(&parts), OURS));
    }

    #[test]
    fn a_consumer_without_a_client_still_counts_when_it_is_not_us() {
        let d = dump(&[
            node(145, "Video/Source", PORTAL, None, 59),
            r#"{"id": 181, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Input/Video"}}}"#.to_owned(),
            link(182, 145, 181, "active"),
        ]);
        assert!(external_capture(&d, OURS));
    }
}
