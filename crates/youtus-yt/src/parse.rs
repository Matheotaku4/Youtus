use std::collections::BTreeSet;

use serde_json::Value;
use youtus_core::{parse_duration, parse_view_count, Feed, Video, VideoId};

/// Résultat d'un `like/like`. `Unknown` couvre le 200 sans `likeStatus` que
/// renvoie parfois le client TV : on ne peut pas le compter comme validé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LikeStatus {
    Liked,
    NotLiked,
    Unknown,
}

pub fn parse_like_status(value: &Value) -> LikeStatus {
    let Some(status) = find(value, "likeStatus").and_then(text_of) else {
        return LikeStatus::Unknown;
    };
    if status.eq_ignore_ascii_case("LIKE") {
        LikeStatus::Liked
    } else if status.eq_ignore_ascii_case("DISLIKE") || status.eq_ignore_ascii_case("INDIFFERENT")
    {
        LikeStatus::NotLiked
    } else {
        LikeStatus::Unknown
    }
}

/// Lit une réponse `browse`/`next` quelle que soit la famille de rendu.
///
/// WEB rend `videoRenderer` puis `lockupViewModel`, la TV rend `tileRenderer`.
/// Un parseur qui ne connaît qu'une famille compte zéro vidéo sur les autres —
/// c'est exactement l'erreur que S1 a mise au jour.
pub fn parse_feed(value: &Value) -> Feed {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    collect(value, &mut items, &mut seen);

    Feed {
        items,
        continuation: find_continuation(value),
    }
}

fn collect(value: &Value, items: &mut Vec<Video>, seen: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    "videoRenderer" => push(parse_video_renderer(child), items, seen),
                    "lockupViewModel" => push(parse_lockup(child), items, seen),
                    "tileRenderer" => push(parse_tile(child), items, seen),
                    _ => collect(child, items, seen),
                }
            }
        }
        Value::Array(list) => {
            for child in list {
                collect(child, items, seen);
            }
        }
        _ => {}
    }
}

fn push(video: Option<Video>, items: &mut Vec<Video>, seen: &mut BTreeSet<String>) {
    if let Some(video) = video {
        if seen.insert(video.id.as_str().to_owned()) {
            items.push(video);
        }
    }
}

fn parse_tile(node: &Value) -> Option<Video> {
    let id = VideoId::new(text_of(find(node, "contentId")?)?)?;
    let metadata = find(node, "tileMetadataRenderer");

    let title = metadata
        .and_then(|node| find(node, "title"))
        .and_then(text_of)
        .unwrap_or_default();
    let lines = metadata.map(line_texts).unwrap_or_default();

    let mut video = Video::new(id, title);
    video.channel = lines.iter().find(|line| is_channel(line)).cloned();
    video.views = lines.iter().find_map(|line| parse_view_count(line));
    video.published = lines.iter().find(|line| is_date(line)).cloned();
    video.duration = find(node, "thumbnailOverlayTimeStatusRenderer")
        .and_then(|status| find(status, "text"))
        .and_then(text_of)
        .and_then(|text| parse_duration(&text));
    video.thumbnail = best_thumbnail(node);
    Some(video)
}

fn parse_video_renderer(node: &Value) -> Option<Video> {
    let id = VideoId::new(text_of(find(node, "videoId")?)?)?;
    let title = find(node, "title").and_then(text_of).unwrap_or_default();

    let mut video = Video::new(id, title);
    video.channel = find(node, "ownerText").and_then(text_of);
    video.channel_id = find(node, "browseEndpoint")
        .and_then(|endpoint| endpoint.get("browseId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    video.duration = find(node, "lengthText")
        .and_then(text_of)
        .and_then(|text| parse_duration(&text));
    video.views = find(node, "viewCountText")
        .and_then(text_of)
        .and_then(|text| parse_view_count(&text));
    video.published = find(node, "publishedTimeText").and_then(text_of);
    video.thumbnail = best_thumbnail(node);
    Some(video)
}

fn parse_lockup(node: &Value) -> Option<Video> {
    let id = VideoId::new(text_of(find(node, "contentId")?)?)?;
    let metadata = find(node, "lockupMetadataViewModel");

    let title = metadata
        .and_then(|node| find(node, "title"))
        .and_then(text_of)
        .unwrap_or_default();
    let texts = metadata.map(metadata_texts).unwrap_or_default();

    let mut video = Video::new(id, title);
    video.channel = texts.iter().find(|line| is_channel(line)).cloned();
    video.views = texts.iter().find_map(|line| parse_view_count(line));
    video.published = texts.iter().find(|line| is_date(line)).cloned();
    video.duration = find(node, "thumbnailBadgeViewModel")
        .and_then(|badge| find(badge, "text"))
        .and_then(text_of)
        .and_then(|text| parse_duration(&text));
    video.thumbnail = best_thumbnail(node);
    Some(video)
}

fn line_texts(metadata: &Value) -> Vec<String> {
    let mut items = Vec::new();
    find_all(metadata, "lineItemRenderer", &mut items);
    items
        .into_iter()
        .filter_map(|item| find(item, "text").and_then(text_of))
        .collect()
}

fn metadata_texts(metadata: &Value) -> Vec<String> {
    let mut parts = Vec::new();
    find_all(metadata, "metadataParts", &mut parts);

    let mut texts = Vec::new();
    for part in parts {
        find_all(part, "text", &mut texts);
    }
    texts.into_iter().filter_map(text_of).collect()
}

fn is_date(text: &str) -> bool {
    let text = text.trim().to_lowercase();
    text.starts_with("il y a")
        || text == "hier"
        || text == "avant-hier"
        || text.ends_with(" ago")
        || text == "yesterday"
}

fn is_channel(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed != "•" && !is_date(trimmed) && parse_view_count(trimmed).is_none()
}

fn best_thumbnail(node: &Value) -> Option<String> {
    let mut lists = Vec::new();
    find_all(node, "thumbnails", &mut lists);

    let mut best: Option<(u64, String)> = None;
    for list in lists {
        let Some(entries) = list.as_array() else {
            continue;
        };
        for entry in entries {
            let Some(url) = entry.get("url").and_then(Value::as_str) else {
                continue;
            };
            let width = entry.get("width").and_then(Value::as_u64).unwrap_or(0);
            if best.as_ref().map(|(best, _)| width >= *best).unwrap_or(true) {
                best = Some((width, normalize_url(url)));
            }
        }
    }
    best.map(|(_, url)| url)
}

fn normalize_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        url.to_owned()
    }
}

fn find_continuation(value: &Value) -> Option<String> {
    let mut items = Vec::new();
    find_all(value, "continuationItemRenderer", &mut items);

    for item in items {
        if let Some(token) = find(item, "continuationCommand")
            .and_then(|command| command.get("token"))
            .and_then(Value::as_str)
        {
            return Some(token.to_owned());
        }
    }

    find(value, "continuationCommand")
        .and_then(|command| command.get("token"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(crate) fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|child| find(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find(child, key)),
        _ => None,
    }
}

fn find_all<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                if child_key == key {
                    out.push(child);
                }
                find_all(child, key, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                find_all(child, key, out);
            }
        }
        _ => {}
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => {
            if let Some(text) = map.get("simpleText").and_then(Value::as_str) {
                return Some(text.to_owned());
            }
            if let Some(text) = map.get("content").and_then(Value::as_str) {
                return Some(text.to_owned());
            }
            if let Some(runs) = map.get("runs").and_then(Value::as_array) {
                let joined: String = runs
                    .iter()
                    .filter_map(|run| run.get("text").and_then(Value::as_str))
                    .collect();
                if !joined.trim().is_empty() {
                    return Some(joined);
                }
            }
            map.get("accessibility")
                .and_then(|node| find(node, "label"))
                .and_then(text_of)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tuile_tv() -> Value {
        json!({
            "tileRenderer": {
                "header": { "tileHeaderRenderer": {
                    "thumbnail": { "thumbnails": [
                        { "url": "//i.ytimg.com/vi/aaa/hq.jpg", "width": 480, "height": 270 },
                        { "url": "https://i.ytimg.com/vi/aaa/max.jpg", "width": 1280, "height": 720 }
                    ]},
                    "thumbnailOverlays": [{ "thumbnailOverlayTimeStatusRenderer": {
                        "text": { "simpleText": "25:35" }, "style": "DEFAULT"
                    }}]
                }},
                "metadata": { "tileMetadataRenderer": {
                    "title": { "simpleText": "Titre TV" },
                    "lines": [
                        { "lineRenderer": { "items": [{ "lineItemRenderer": {
                            "text": { "runs": [{ "text": "Chaîne TV" }] }
                        }}]}},
                        { "lineRenderer": { "items": [
                            { "lineItemRenderer": { "text": { "simpleText": "1,2 M de vues" } } },
                            { "lineItemRenderer": { "text": { "simpleText": "•" } } },
                            { "lineItemRenderer": { "text": { "simpleText": "il y a 1 jour" } } }
                        ]}}
                    ]
                }},
                "contentId": "dQw4w9WgXcQ",
                "contentType": "VIDEO"
            }
        })
    }

    fn renderer_web() -> Value {
        json!({
            "videoRenderer": {
                "videoId": "abcdefghijk",
                "title": { "runs": [{ "text": "Titre " }, { "text": "web" }] },
                "ownerText": { "runs": [{ "text": "Chaîne web" }] },
                "lengthText": { "simpleText": "1:02:03" },
                "viewCountText": { "simpleText": "12 345 vues" },
                "publishedTimeText": { "simpleText": "il y a 2 semaines" },
                "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/bbb/hq.jpg", "width": 360 }] },
                "ownerTextRuns": [{ "navigationEndpoint": {
                    "browseEndpoint": { "browseId": "UC12345678901234567890" }
                }}]
            }
        })
    }

    fn lockup_web() -> Value {
        json!({
            "lockupViewModel": {
                "contentId": "lmnopqrstuv",
                "metadata": { "lockupMetadataViewModel": {
                    "title": { "content": "Titre lockup" },
                    "metadataRows": [
                        { "metadataParts": [
                            { "text": { "content": "Chaîne lockup" } },
                            { "text": { "content": "9,5 k de vues" } }
                        ]},
                        { "metadataParts": [{ "text": { "content": "il y a 3 jours" } }] }
                    ]
                }},
                "contentImage": { "thumbnailViewModel": {
                    "thumbnails": [{ "url": "//i.ytimg.com/vi/ccc/hq.jpg", "width": 400 }],
                    "overlays": [{ "thumbnailOverlayBadgeViewModel": { "thumbnailBadges": [
                        { "thumbnailBadgeViewModel": { "text": "4:20" } }
                    ]}}]
                }}
            }
        })
    }

    #[test]
    fn lit_une_tuile_tv() {
        let feed = parse_feed(&tuile_tv());
        assert_eq!(feed.len(), 1);
        let video = &feed.items[0];
        assert_eq!(video.id.as_str(), "dQw4w9WgXcQ");
        assert_eq!(video.title, "Titre TV");
        assert_eq!(video.channel.as_deref(), Some("Chaîne TV"));
        assert_eq!(video.views, Some(1_200_000));
        assert_eq!(video.published.as_deref(), Some("il y a 1 jour"));
        assert_eq!(video.duration, Some(std::time::Duration::from_secs(1535)));
        assert_eq!(
            video.thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/aaa/max.jpg")
        );
    }

    #[test]
    fn lit_un_video_renderer_web() {
        let feed = parse_feed(&renderer_web());
        let video = &feed.items[0];
        assert_eq!(video.id.as_str(), "abcdefghijk");
        assert_eq!(video.title, "Titre web");
        assert_eq!(video.channel.as_deref(), Some("Chaîne web"));
        assert_eq!(video.views, Some(12_345));
        assert_eq!(video.duration, Some(std::time::Duration::from_secs(3723)));
        assert_eq!(video.published.as_deref(), Some("il y a 2 semaines"));
        assert_eq!(
            video.channel_id.as_deref(),
            Some("UC12345678901234567890"),
            "l'identifiant de chaîne doit survivre au parseur"
        );
    }

    #[test]
    fn lit_un_lockup_web() {
        let feed = parse_feed(&lockup_web());
        let video = &feed.items[0];
        assert_eq!(video.id.as_str(), "lmnopqrstuv");
        assert_eq!(video.title, "Titre lockup");
        assert_eq!(video.channel.as_deref(), Some("Chaîne lockup"));
        assert_eq!(video.views, Some(9_500));
        assert_eq!(video.published.as_deref(), Some("il y a 3 jours"));
        assert_eq!(video.duration, Some(std::time::Duration::from_secs(260)));
        assert_eq!(
            video.thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/ccc/hq.jpg")
        );
    }

    #[test]
    fn lit_les_trois_familles_dans_une_meme_reponse() {
        let value = json!({
            "contents": {
                "a": tuile_tv(),
                "b": renderer_web(),
                "c": lockup_web(),
                "continuationItemRenderer": {
                    "continuationEndpoint": { "continuationCommand": { "token": "SUITE" } }
                }
            }
        });
        let feed = parse_feed(&value);
        assert_eq!(feed.len(), 3);
        assert_eq!(feed.continuation.as_deref(), Some("SUITE"));
    }

    #[test]
    fn ignore_les_doublons() {
        let value = json!({ "a": tuile_tv(), "b": tuile_tv() });
        assert_eq!(parse_feed(&value).len(), 1);
    }

    #[test]
    fn une_reponse_vide_ne_panique_pas() {
        let feed = parse_feed(&json!({}));
        assert!(feed.is_empty());
        assert!(feed.continuation.is_none());
    }

    #[test]
    fn un_like_sans_like_status_reste_inconnu() {
        assert_eq!(parse_like_status(&json!({})), LikeStatus::Unknown);
        assert_eq!(
            parse_like_status(&json!({ "likeStatus": "LIKE" })),
            LikeStatus::Liked
        );
        assert_eq!(
            parse_like_status(&json!({ "likeStatus": "INDIFFERENT" })),
            LikeStatus::NotLiked
        );
        assert_eq!(
            parse_like_status(&json!({ "likeStatus": "AUTRE" })),
            LikeStatus::Unknown
        );
    }

    #[test]
    fn un_identifiant_invalide_est_ignore() {
        let value = json!({ "tileRenderer": { "contentId": "court", "metadata": {} } });
        assert!(parse_feed(&value).is_empty());
    }
}
