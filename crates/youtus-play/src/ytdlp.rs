//! Résolution du flux par `yt-dlp -J`. On ne réimplémente ni SABR ni les PO
//! tokens : yt-dlp reste la seule source d'URL lisibles.

use std::path::Path;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedStream {
    pub url: String,
    /// Second flux quand vidéo et audio sont séparés (DASH).
    pub audio_url: Option<String>,
    pub http_headers: Vec<(String, String)>,
    pub format_id: String,
    pub height: Option<u32>,
    pub duration: Option<f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("yt-dlp introuvable : installe-le (paquet `yt-dlp` ou binaire standalone)")]
    YtDlpMissing,

    #[error("yt-dlp a échoué : {0}")]
    Failed(String),

    #[error("sortie yt-dlp illisible : {0}")]
    Json(String),

    #[error("aucun flux lisible : {0}")]
    NoStream(String),

    #[error("les directs ne sont pas pris en charge")]
    Live,
}

/// `max_height` borne la définition demandée à yt-dlp. Les clients tv/ios sont
/// forcés : avec les clients par défaut, yt-dlp rend des URLs DASH que le CDN
/// refuse en 403 (signature liée au client d'origine).
pub fn resolve(
    video_id: &str,
    max_height: u32,
    cookies: Option<&Path>,
) -> Result<ResolvedStream, Error> {
    let format = format!("b[height<={max_height}]/b");
    let mut command = std::process::Command::new("yt-dlp");
    command.args([
        "-J",
        "--no-warnings",
        "--no-playlist",
        "--no-progress",
        "-f",
        &format,
        "--extractor-args",
        "youtube:player-client=tv,ios,web_safari",
    ]);
    if let Some(cookies) = cookies {
        command.arg("--cookies").arg(cookies);
    }
    command.arg(format!("https://www.youtube.com/watch?v={video_id}"));

    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::YtDlpMissing
        } else {
            Error::Failed(error.to_string())
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("échec sans message")
            .to_owned();
        return Err(Error::Failed(message));
    }

    parse(&String::from_utf8_lossy(&output.stdout))
}

fn parse(text: &str) -> Result<ResolvedStream, Error> {
    let value: Value = serde_json::from_str(text).map_err(|error| Error::Json(error.to_string()))?;

    if value.get("is_live").and_then(Value::as_bool) == Some(true) {
        return Err(Error::Live);
    }

    let duration = value.get("duration").and_then(Value::as_f64);
    let requested: Vec<&Value> = value
        .get("requested_formats")
        .and_then(Value::as_array)
        .map(|list| list.iter().collect())
        .unwrap_or_default();

    let (url, audio_url, format_id, height, headers) = if requested.is_empty() {
        let url = value
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::NoStream("`url` absent".to_owned()))?;
        (
            url.to_owned(),
            None,
            text_field(&value, "format_id"),
            value.get("height").and_then(Value::as_u64).map(|h| h as u32),
            headers_of(&value),
        )
    } else {
        let video = requested
            .iter()
            .find(|format| format.get("vcodec").and_then(Value::as_str) != Some("none"))
            .ok_or_else(|| Error::NoStream("aucun flux vidéo dans requested_formats".to_owned()))?;
        let audio = requested
            .iter()
            .find(|format| format.get("acodec").and_then(Value::as_str) != Some("none"));

        let url = video
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::NoStream("`url` vidéo absent".to_owned()))?;
        (
            url.to_owned(),
            audio
                .and_then(|format| format.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            text_field(video, "format_id"),
            video.get("height").and_then(Value::as_u64).map(|h| h as u32),
            headers_of(video),
        )
    };

    Ok(ResolvedStream {
        url,
        audio_url,
        http_headers: headers,
        format_id,
        height,
        duration,
    })
}

fn headers_of(value: &Value) -> Vec<(String, String)> {
    value
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .map(|value| (name.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lit_un_flux_unique() {
        let stream = parse(
            &json!({
                "format_id": "95",
                "url": "https://example.test/video.m3u8",
                "height": 720,
                "duration": 213.0,
                "http_headers": { "User-Agent": "UA" },
                "is_live": false
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(stream.format_id, "95");
        assert_eq!(stream.url, "https://example.test/video.m3u8");
        assert_eq!(stream.audio_url, None);
        assert_eq!(stream.height, Some(720));
        assert_eq!(stream.duration, Some(213.0));
        assert_eq!(
            stream.http_headers,
            vec![("User-Agent".to_owned(), "UA".to_owned())]
        );
    }

    #[test]
    fn assemble_video_et_audio() {
        let stream = parse(
            &json!({
                "duration": 10.0,
                "requested_formats": [
                    { "format_id": "137", "vcodec": "avc1", "acodec": "none",
                      "url": "https://example.test/v.mp4", "height": 1080,
                      "http_headers": { "User-Agent": "UA" } },
                    { "format_id": "140", "vcodec": "none", "acodec": "mp4a",
                      "url": "https://example.test/a.m4a" }
                ]
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(stream.url, "https://example.test/v.mp4");
        assert_eq!(stream.audio_url.as_deref(), Some("https://example.test/a.m4a"));
        assert_eq!(stream.format_id, "137");
        assert_eq!(stream.height, Some(1080));
    }

    #[test]
    fn refuse_un_direct() {
        assert!(matches!(
            parse(&json!({ "is_live": true, "url": "x" }).to_string()),
            Err(Error::Live)
        ));
    }

    #[test]
    fn refuse_un_json_sans_url() {
        assert!(matches!(
            parse(&json!({ "format_id": "18" }).to_string()),
            Err(Error::NoStream(_))
        ));
        assert!(matches!(parse("pas du json"), Err(Error::Json(_))));
    }
}
