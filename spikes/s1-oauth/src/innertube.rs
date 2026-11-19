use anyhow::{bail, Result};
use serde_json::{json, Map, Value};

use crate::json_util::find_string;

pub const WEB_CLIENT_VERSION: &str = "2.20261101.00.00";
pub const TV_CLIENT_VERSION: &str = "7.20250101.00.00";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClientKind {
    Web,
    Tv,
}

impl ClientKind {
    pub fn label(self) -> &'static str {
        match self {
            ClientKind::Web => "WEB",
            ClientKind::Tv => "TVHTML5",
        }
    }

    fn version(self) -> String {
        let (default, env) = match self {
            ClientKind::Web => (WEB_CLIENT_VERSION, "YOUTUS_WEB_VERSION"),
            ClientKind::Tv => (TV_CLIENT_VERSION, "YOUTUS_TV_VERSION"),
        };
        std::env::var(env).unwrap_or_else(|_| default.to_owned())
    }

    fn client_name_header(self) -> &'static str {
        match self {
            ClientKind::Web => "1",
            ClientKind::Tv => "7",
        }
    }

    fn user_agent(self) -> &'static str {
        match self {
            ClientKind::Web => {
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/140.0.0.0 Safari/537.36"
            }
            ClientKind::Tv => "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.1",
        }
    }
}

pub struct Innertube {
    agent: ureq::Agent,
    pub kind: ClientKind,
    visitor: Option<String>,
    token: Option<String>,
}

impl Innertube {
    pub fn new(kind: ClientKind, token: Option<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(kind.user_agent())
            .timeout(std::time::Duration::from_secs(30))
            .build();
        Self {
            agent,
            kind,
            visitor: None,
            token,
        }
    }

    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    fn context(&self) -> Value {
        let mut client = Map::new();
        client.insert("clientName".into(), json!(self.kind.label()));
        client.insert("clientVersion".into(), json!(self.kind.version()));
        client.insert("hl".into(), json!("fr"));
        client.insert("gl".into(), json!("FR"));
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
            .set("X-YouTube-Client-Name", self.kind.client_name_header())
            .set("X-YouTube-Client-Version", &self.kind.version());

        if self.kind == ClientKind::Web {
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
