use anyhow::{bail, Result};
use serde_json::{json, Map, Value};

use crate::json_util::find_string;

pub const WEB_CLIENT_VERSION: &str = "2.20261101.00.00";
pub const TV_CLIENT_VERSION: &str = "7.20260707.07.00";
pub const TV_DOWNGRADED_VERSION: &str = "5.20260707";
pub const TV_PREVIOUS_VERSION: &str = "7.20250209.19.00";

const CHROME_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
                         Chrome/140.0.0.0 Safari/537.36";
const COBALT_UA: &str = "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.30.1034943-gold \
                         (unlike Gecko), Unknown_TV_Unknown_0/Unknown (Unknown, Unknown)";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Web,
    Tv,
}

/// Un couple contexte InnerTube + en-têtes. Le jeton OAuth du device flow est
/// lié à un client TV : les profils sont essayés dans l'ordre jusqu'à acceptation.
pub struct Profile {
    pub label: &'static str,
    pub client_name: &'static str,
    pub version: String,
    pub header_name: &'static str,
    pub user_agent: &'static str,
    pub platform: Option<&'static str>,
    pub family: Family,
}

pub fn profiles(family: Option<Family>) -> Vec<Profile> {
    let web_version =
        std::env::var("YOUTUS_WEB_VERSION").unwrap_or_else(|_| WEB_CLIENT_VERSION.to_owned());
    let tv_version =
        std::env::var("YOUTUS_TV_VERSION").unwrap_or_else(|_| TV_CLIENT_VERSION.to_owned());

    let all = vec![
        Profile {
            label: "WEB",
            client_name: "WEB",
            version: web_version,
            header_name: "1",
            user_agent: CHROME_UA,
            platform: None,
            family: Family::Web,
        },
        Profile {
            label: "TVHTML5 7.x",
            client_name: "TVHTML5",
            version: tv_version,
            header_name: "7",
            user_agent: COBALT_UA,
            platform: Some("TV"),
            family: Family::Tv,
        },
        Profile {
            label: "TVHTML5 5.x (downgraded)",
            client_name: "TVHTML5",
            version: TV_DOWNGRADED_VERSION.to_owned(),
            header_name: "7",
            user_agent: COBALT_UA,
            platform: Some("TV"),
            family: Family::Tv,
        },
        Profile {
            label: "TVHTML5 7.20250209",
            client_name: "TVHTML5",
            version: TV_PREVIOUS_VERSION.to_owned(),
            header_name: "7",
            user_agent: COBALT_UA,
            platform: Some("TV"),
            family: Family::Tv,
        },
    ];

    match family {
        Some(family) => all.into_iter().filter(|p| p.family == family).collect(),
        None => all,
    }
}

pub struct Innertube {
    agent: ureq::Agent,
    pub profile: Profile,
    visitor: Option<String>,
    token: Option<String>,
}

impl Innertube {
    pub fn new(profile: Profile, token: Option<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(profile.user_agent)
            .timeout(std::time::Duration::from_secs(30))
            .build();
        Self {
            agent,
            profile,
            visitor: None,
            token,
        }
    }

    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    fn context(&self) -> Value {
        let mut client = Map::new();
        client.insert("clientName".into(), json!(self.profile.client_name));
        client.insert("clientVersion".into(), json!(self.profile.version));
        client.insert("hl".into(), json!("fr"));
        client.insert("gl".into(), json!("FR"));
        if let Some(platform) = self.profile.platform {
            client.insert("platform".into(), json!(platform));
        }
        if let Some(visitor) = &self.visitor {
            client.insert("visitorData".into(), json!(visitor));
        }
        json!({ "client": Value::Object(client) })
    }

    pub fn post(&mut self, endpoint: &str, mut body: Value) -> Result<Value> {
        if let Value::Object(map) = &mut body {
            map.insert("context".into(), self.context());
        }

        let url = format!("https://www.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false");
        let mut request = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("X-YouTube-Client-Name", self.profile.header_name)
            .set("X-YouTube-Client-Version", &self.profile.version);

        if self.profile.family == Family::Web {
            request = request
                .set("Origin", "https://www.youtube.com")
                .set("Referer", "https://www.youtube.com/");
        }
        if let Some(visitor) = &self.visitor {
            request = request.set("X-Goog-Visitor-Id", visitor);
        }
        if let Some(token) = &self.token {
            request = request
                .set("Authorization", &format!("Bearer {token}"))
                .set("X-Goog-AuthUser", "0");
        }

        let value = match request.send_json(body) {
            Ok(response) => response.into_json::<Value>()?,
            Err(ureq::Error::Status(code, response)) => {
                let text = response.into_string().unwrap_or_default();
                let snippet: String = text.chars().take(300).collect();
                bail!("HTTP {code} sur /{endpoint} : {snippet}");
            }
            Err(error) => return Err(error.into()),
        };

        if self.visitor.is_none() {
            if let Some(visitor) = value
                .get("responseContext")
                .and_then(|context| context.get("visitorData"))
                .and_then(Value::as_str)
            {
                self.visitor = Some(visitor.to_owned());
            } else if let Some(visitor) = find_string(&value, "visitorData") {
                self.visitor = Some(visitor);
            }
        }

        Ok(value)
    }
}
