use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::innertube::Innertube;
use crate::json_util::{count_key, find_key, find_string};

pub struct AccountProbe {
    pub name: Option<String>,
}

pub struct FeedProbe {
    pub videos: usize,
    pub feed_nudge: bool,
}

pub struct PlayerProbe {
    pub status: String,
    pub title: Option<String>,
    pub watchtime: bool,
    pub playback: bool,
    pub atr: bool,
    pub sts: Option<u32>,
    pub detail: String,
}

pub struct CommentsProbe {
    pub token_found: bool,
    pub params: Option<String>,
    pub comments: usize,
}

pub struct LikeProbe {
    pub liked: bool,
    pub restored: bool,
    pub detail: String,
}

pub fn probe_account(yt: &mut Innertube) -> Result<AccountProbe> {
    let value = yt.post("account/account_menu", json!({}))?;
    let name = find_string(&value, "accountName")
        .or_else(|| find_string(&value, "email"))
        .or_else(|| find_string(&value, "channelHandle"));
    Ok(AccountProbe { name })
}

pub fn probe_home(yt: &mut Innertube) -> Result<FeedProbe> {
    let value = yt.post("browse", json!({ "browseId": "FEwhat_to_watch" }))?;
    let videos = count_key(&value, "videoRenderer").max(count_key(&value, "lockupViewModel"));
    Ok(FeedProbe {
        videos,
        feed_nudge: count_key(&value, "feedNudgeRenderer") > 0,
    })
}

pub fn probe_history(yt: &mut Innertube) -> Result<usize> {
    let value = yt.post("browse", json!({ "browseId": "FEhistory" }))?;
    Ok(count_key(&value, "videoRenderer").max(count_key(&value, "lockupViewModel")))
}

pub fn probe_player(yt: &mut Innertube, video: &str) -> Result<PlayerProbe> {
    let sts = signature_timestamp(yt.agent(), video).ok();

    let mut content = json!({ "html5Preference": "HTML5_PREF_WANTS" });
    if let Some(sts) = sts {
        content["signatureTimestamp"] = json!(sts);
    }
    let body = json!({
        "videoId": video,
        "contentCheckOk": true,
        "racyCheckOk": true,
        "playbackContext": { "contentPlaybackContext": content },
    });

    let value = yt.post("player", body)?;
    let status = find_string(&value, "status").unwrap_or_else(|| "inconnu".into());
    let title = value
        .get("videoDetails")
        .and_then(|details| details.get("title"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let tracking = value.get("playbackTracking");
    let has_url = |key: &str| {
        tracking
            .and_then(|tracking| tracking.get(key))
            .and_then(|node| node.get("baseUrl"))
            .and_then(Value::as_str)
            .map(|url| !url.is_empty())
            .unwrap_or(false)
    };

    let watchtime = has_url("videostatsWatchtimeUrl");
    let playback = has_url("videostatsPlaybackUrl");
    let atr = has_url("atrUrl");

    let detail = match (tracking.is_some(), sts.is_some()) {
        (false, false) => "aucun bloc playbackTracking, sts introuvable".to_owned(),
        (false, true) => "aucun bloc playbackTracking malgré un sts valide".to_owned(),
        (true, false) => "bloc présent sans sts (à interpréter)".to_owned(),
        (true, true) => format!("bloc présent (sts {})", sts.unwrap_or_default()),
    };

    Ok(PlayerProbe {
        status,
        title,
        watchtime,
        playback,
        atr,
        sts,
        detail,
    })
}

pub fn probe_comments(yt: &mut Innertube, video: &str) -> Result<CommentsProbe> {
    let next = yt.post("next", json!({ "videoId": video }))?;
    let token = find_comment_continuation(&next);
    let Some(token) = token else {
        return Ok(CommentsProbe {
            token_found: false,
            params: None,
            comments: 0,
        });
    };

    let comments = yt.post("next", json!({ "continuation": token }))?;
    Ok(CommentsProbe {
        token_found: true,
        params: find_create_comment_params(&comments),
        comments: count_key(&comments, "commentEntityPayload")
            + count_key(&comments, "commentThreadRenderer"),
    })
}

pub fn probe_like(yt: &mut Innertube, video: &str) -> Result<LikeProbe> {
    let target = json!({ "target": { "videoId": video } });
    let liked_response = yt.post("like/like", target.clone())?;

    let liked = find_key(&liked_response, "likeStatus")
        .and_then(Value::as_str)
        .map(|status| status.eq_ignore_ascii_case("like"))
        .unwrap_or_else(|| liked_response.to_string().contains("\"LIKE\""));

    // On remet systématiquement l'état d'origine (aucune note).
    let restored = yt.post("like/removelike", target).is_ok();

    Ok(LikeProbe {
        liked,
        restored,
        detail: if liked {
            "likeStatus = LIKE".to_owned()
        } else {
            "HTTP 200 sans likeStatus explicite".to_owned()
        },
    })
}

pub fn find_comment_continuation(next: &Value) -> Option<String> {
    fn walk(value: &Value, in_comments: bool, found: &mut Option<String>) {
        if found.is_some() {
            return;
        }
        match value {
            Value::Object(map) => {
                let in_comments = in_comments
                    || map.get("panelIdentifier").and_then(Value::as_str)
                        == Some("comment-item-section");
                if in_comments {
                    if let Some(token) = map
                        .get("continuationCommand")
                        .and_then(|command| command.get("token"))
                        .and_then(Value::as_str)
                    {
                        *found = Some(token.to_owned());
                        return;
                    }
                }
                for child in map.values() {
                    walk(child, in_comments, found);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, in_comments, found);
                }
            }
            _ => {}
        }
    }

    let mut found = None;
    walk(next, false, &mut found);
    found.or_else(|| {
        find_key(next, "continuationCommand")
            .and_then(|command| command.get("token"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

pub fn find_create_comment_params(comments: &Value) -> Option<String> {
    find_key(comments, "createCommentParams")
        .and_then(Value::as_str)
        .filter(|params| !params.is_empty())
        .map(str::to_owned)
}

/// Extrait le `signatureTimestamp` du player JS courant, sans lequel YouTube
/// n'inclut pas `playbackTracking` dans /player.
pub fn signature_timestamp(agent: &ureq::Agent, video: &str) -> Result<u32> {
    let html = agent
        .get(&format!("https://www.youtube.com/watch?v={video}"))
        .call()
        .context("GET watch")?
        .into_string()?;

    let js_url = extract_js_url(&html).ok_or_else(|| anyhow!("jsUrl introuvable"))?;
    let js_url = if js_url.starts_with("http") {
        js_url
    } else {
        format!("https://www.youtube.com{js_url}")
    };

    let js = agent
        .get(&js_url)
        .call()
        .with_context(|| format!("GET {js_url}"))?
        .into_string()?;

    extract_sts(&js).ok_or_else(|| anyhow!("signatureTimestamp introuvable dans {js_url}"))
}

fn extract_js_url(html: &str) -> Option<String> {
    for marker in ["\"jsUrl\":\"", "\"PLAYER_JS_URL\":\""] {
        if let Some(start) = html.find(marker) {
            let rest = &html[start + marker.len()..];
            let end = rest.find('"')?;
            return Some(rest[..end].replace("\\/", "/"));
        }
    }
    None
}

fn extract_sts(js: &str) -> Option<u32> {
    for marker in ["signatureTimestamp:", "signatureTimestamp="] {
        let mut cursor = 0;
        while let Some(offset) = js[cursor..].find(marker) {
            let start = cursor + offset + marker.len();
            let digits: String = js[start..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(sts) = digits.parse() {
                return Some(sts);
            }
            cursor = start;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrait_le_js_url() {
        let html = r#"{"jsUrl":"\/s\/player\/abc\/player_ias.vflset\/fr_FR\/base.js"}"#;
        assert_eq!(
            extract_js_url(html).as_deref(),
            Some("/s/player/abc/player_ias.vflset/fr_FR/base.js")
        );
    }

    #[test]
    fn extrait_le_sts() {
        assert_eq!(extract_sts("var a={signatureTimestamp:20123,b:1}"), Some(20123));
        assert_eq!(extract_sts("signatureTimestamp=20777;"), Some(20777));
        assert_eq!(extract_sts("rien"), None);
    }

    #[test]
    fn trouve_la_continuation_des_commentaires() {
        let next = json!({
            "engagementPanels": [
                {"panelIdentifier": "engagement-panel-related", "continuationCommand": {"token": "MAUVAIS"}},
                {"panelIdentifier": "comment-item-section", "header": {"continuationCommand": {"token": "BON"}}}
            ]
        });
        assert_eq!(find_comment_continuation(&next).as_deref(), Some("BON"));
    }

    #[test]
    fn retombe_sur_la_premiere_continuation() {
        let next = json!({"a": {"continuationCommand": {"token": "SEUL"}}});
        assert_eq!(find_comment_continuation(&next).as_deref(), Some("SEUL"));
    }

    #[test]
    fn lit_create_comment_params() {
        let comments = json!({
            "x": {"createCommentEndpoint": {"createCommentParams": "EhgKEA"}}
        });
        assert_eq!(find_create_comment_params(&comments).as_deref(), Some("EhgKEA"));
        assert_eq!(
            find_create_comment_params(&json!({"createCommentParams": ""})),
            None
        );
    }
}
