/// Versions de client InnerTube. Elles changent souvent : c'est la première
/// chose à ajuster quand YouTube renvoie 400 sur des requêtes pourtant valides.
pub const WEB_CLIENT_VERSION: &str = "2.20261101.00.00";
pub const TV_CLIENT_VERSION: &str = "7.20260707.07.00";

pub const CHROME_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
                             Chrome/140.0.0.0 Safari/537.36";
pub const COBALT_UA: &str = "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.30.1034943-gold \
                             (unlike Gecko), Unknown_TV_Unknown_0/Unknown (Unknown, Unknown)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Web,
    Tv,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub family: Family,
    pub client_name: &'static str,
    pub client_version: String,
    pub header_name: &'static str,
    pub user_agent: &'static str,
    pub platform: Option<&'static str>,
}

impl Profile {
    pub fn web() -> Self {
        Self {
            family: Family::Web,
            client_name: "WEB",
            client_version: WEB_CLIENT_VERSION.to_owned(),
            header_name: "1",
            user_agent: CHROME_UA,
            platform: None,
        }
    }

    pub fn tvhtml5() -> Self {
        Self {
            family: Family::Tv,
            client_name: "TVHTML5",
            client_version: TV_CLIENT_VERSION.to_owned(),
            header_name: "7",
            user_agent: COBALT_UA,
            platform: Some("TV"),
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.client_version = version.into();
        self
    }

    pub fn is_web(&self) -> bool {
        self.family == Family::Web
    }

    pub fn is_tv(&self) -> bool {
        self.family == Family::Tv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_et_tv_ont_des_en_tetes_distincts() {
        let web = Profile::web();
        let tv = Profile::tvhtml5();
        assert_eq!(web.header_name, "1");
        assert_eq!(tv.header_name, "7");
        assert_eq!(tv.platform, Some("TV"));
        assert!(tv.is_tv() && !tv.is_web());
    }

    #[test]
    fn la_version_est_remplacable() {
        let profile = Profile::tvhtml5().with_version("7.20250101.00.00");
        assert_eq!(profile.client_version, "7.20250101.00.00");
        assert_eq!(profile.client_name, "TVHTML5");
    }
}
