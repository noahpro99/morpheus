//! Link previews: what the homeserver's URL preview API
//! (`/_matrix/client/v1/media/preview_url`) says a page is, so this device
//! never contacts the site and the site never sees it.

use matrix_sdk::ruma::api::client::authenticated_media::get_media_preview;
use serde_json::Value;

use crate::{CoreFuture, MediaRef, Session};

/// A page as a card: the site's name, the title, a description and a
/// picture (the page's `og:image`, kept on the homeserver).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlPreview {
    pub url: String,
    pub site: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<MediaRef>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
}

/// The card for `url` from the preview API's answer (Open Graph properties
/// as one JSON object); `None` when there is nothing to show.
pub fn parse_preview(url: &str, data: &Value) -> Option<UrlPreview> {
    let text = |key: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
    };
    let number = |key: &str| {
        data.get(key).and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .and_then(|n| u32::try_from(n).ok())
        })
    };
    let preview = UrlPreview {
        url: url.to_owned(),
        site: text("og:site_name"),
        title: text("og:title"),
        description: text("og:description"),
        image: data
            .get("og:image")
            .and_then(Value::as_str)
            .filter(|u| u.starts_with("mxc://"))
            .map(MediaRef::from_mxc_uri),
        image_width: number("og:image:width"),
        image_height: number("og:image:height"),
    };
    (preview.title.is_some() || preview.description.is_some() || preview.image.is_some())
        .then_some(preview)
}

impl Session {
    /// The preview of the page at `url`, asked of the homeserver (`None`
    /// when it has nothing, does not do previews, or the page is not one).
    pub fn url_preview(&self, url: String) -> CoreFuture<Option<UrlPreview>> {
        let client = self.client.clone();
        self.core.run(async move {
            let request = get_media_preview::v1::Request::new(url.clone());
            let Ok(response) = client.send(request).await else {
                return Ok(None);
            };
            let Some(data) = response.data else {
                return Ok(None);
            };
            Ok(serde_json::from_str::<Value>(data.get())
                .ok()
                .and_then(|data| parse_preview(&url, &data)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_graph_becomes_a_card() {
        let card = parse_preview(
            "https://example.org/a",
            &json!({
                "og:site_name": " Example ",
                "og:title": "A  title\n here",
                "og:description": "Words",
                "og:image": "mxc://hs/abc",
                "og:image:width": 640,
                "og:image:height": "480",
            }),
        )
        .expect("a card");
        assert_eq!(card.site.as_deref(), Some("Example"));
        assert_eq!(card.title.as_deref(), Some("A title here"));
        assert_eq!(card.description.as_deref(), Some("Words"));
        assert_eq!(card.image.as_ref().map(|i| i.key()), Some("mxc://hs/abc"));
        assert_eq!((card.image_width, card.image_height), (Some(640), Some(480)));
    }

    #[test]
    fn nothing_to_show_is_no_card() {
        assert_eq!(parse_preview("https://x", &json!({})), None);
        assert_eq!(parse_preview("https://x", &json!({ "og:title": "  " })), None);
        let only_site = json!({ "og:site_name": "S" });
        assert_eq!(parse_preview("https://x", &only_site), None);
        // A picture on a server that is not ours (not mxc) is not used.
        let remote = parse_preview(
            "https://x",
            &json!({ "og:title": "T", "og:image": "https://x/i.png" }),
        )
        .expect("title alone");
        assert!(remote.image.is_none());
    }
}
