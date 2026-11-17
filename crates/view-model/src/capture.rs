//! Whether another program is capturing the screen (#143): on Linux, from
//! PipeWire's own dump (`pw-dump`): a screen-cast stream (the portal's
//! `Stream/Output/Video`, not a camera) linked, actively, to a program that is
//! not this one (a Discord stream, OBS). Pure: the app asks the system and
//! hands the text here.

use serde_json::Value;

/// Whether `pw_dump` (PipeWire's JSON dump of its graph) has an active
/// screen-cast link to a program other than the one with process id `our_pid`.
pub fn external_capture(pw_dump: &str, our_pid: u32) -> bool {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(pw_dump) else {
        return false;
    };
    let props = |object: &Value| object["info"]["props"].clone();
    let node = |id: u64| {
        objects.iter().find(|o| {
            o["type"] == "PipeWire:Interface:Node" && o["id"].as_u64() == Some(id)
        })
    };
    let text = |p: &Value, key: &str| p[key].as_str().unwrap_or_default().to_owned();
    let pid_of = |p: &Value| {
        p["application.process.id"]
            .as_u64()
            .or_else(|| p["application.process.id"].as_str().and_then(|s| s.parse().ok()))
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
        let (source, reader) = (props(from), props(to));
        // A screen cast: the portal's stream of the screen (a camera's has the
        // role Camera, a device node is `Video/Source`).
        let screen = text(&source, "media.class") == "Stream/Output/Video"
            && text(&source, "media.role") != "Camera";
        let ours = pid_of(&reader) == Some(u64::from(our_pid));
        screen && !ours
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    const OURS: u32 = 4242;

    fn node(id: u32, class: &str, role: Option<&str>, pid: Option<u32>, app: &str) -> String {
        let role = role.map_or(String::new(), |r| format!(r#""media.role": "{r}","#));
        let pid = pid.map_or(String::new(), |p| format!(r#""application.process.id": {p},"#));
        format!(
            r#"{{"id": {id}, "type": "PipeWire:Interface:Node", "info": {{"props": {{{role}{pid}"media.class": "{class}", "application.name": "{app}"}}}}}}"#
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

    #[test]
    fn nothing_linked_is_no_capture() {
        let d = dump(&[node(1, "Stream/Output/Video", Some("Screen"), None, "xdg-desktop-portal")]);
        assert!(!external_capture(&d, OURS));
        assert!(!external_capture("[]", OURS));
        assert!(!external_capture("not json", OURS));
    }

    #[test]
    fn another_program_on_a_screen_cast_is_a_capture() {
        let d = dump(&[
            node(1, "Stream/Output/Video", Some("Screen"), None, "xdg-desktop-portal"),
            node(2, "Stream/Input/Video", None, Some(111), "Discord"),
            link(3, 1, 2, "active"),
        ]);
        assert!(external_capture(&d, OURS));
    }

    #[test]
    fn our_own_capture_is_not_external() {
        let d = dump(&[
            node(1, "Stream/Output/Video", Some("Screen"), None, "xdg-desktop-portal"),
            node(2, "Stream/Input/Video", None, Some(OURS), "morpheus-desktop"),
            link(3, 1, 2, "active"),
        ]);
        assert!(!external_capture(&d, OURS));
    }

    #[test]
    fn a_camera_is_not_a_screen_capture() {
        let d = dump(&[
            node(1, "Stream/Output/Video", Some("Camera"), None, "xdg-desktop-portal"),
            node(2, "Stream/Input/Video", None, Some(111), "Discord"),
            link(3, 1, 2, "active"),
        ]);
        assert!(!external_capture(&d, OURS), "a webcam in a call");
        let device = dump(&[
            node(1, "Video/Source", None, None, "v4l2"),
            node(2, "Stream/Input/Video", None, Some(111), "Discord"),
            link(3, 1, 2, "active"),
        ]);
        assert!(!external_capture(&device, OURS), "a camera device node");
    }

    #[test]
    fn a_paused_or_unlinked_stream_is_not_a_capture() {
        let paused = dump(&[
            node(1, "Stream/Output/Video", Some("Screen"), None, "xdg-desktop-portal"),
            node(2, "Stream/Input/Video", None, Some(111), "OBS"),
            link(3, 1, 2, "paused"),
        ]);
        assert!(!external_capture(&paused, OURS));
    }

    #[test]
    fn ours_and_theirs_together_is_external() {
        let d = dump(&[
            node(1, "Stream/Output/Video", Some("Screen"), None, "xdg-desktop-portal"),
            node(2, "Stream/Input/Video", None, Some(OURS), "morpheus-desktop"),
            link(3, 1, 2, "active"),
            node(4, "Stream/Output/Video", Some("Screen"), None, "xdg-desktop-portal"),
            node(5, "Stream/Input/Video", None, Some(222), "obs"),
            link(6, 4, 5, "active"),
        ]);
        assert!(external_capture(&d, OURS));
    }
}
