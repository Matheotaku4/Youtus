//! Chemins et options de démarrage. Aucune valeur secrète n'est lue ici : les
//! fichiers ne sont ouverts qu'au moment de s'en servir.

use std::path::PathBuf;

use youtus_auth::{data_dir, default_token_path};

pub const USAGE: &str = "\
Youtus — client YouTube pour Linux (Slint + mpv)

USAGE :
    youtus [OPTIONS]

OPTIONS :
    --client-secret-file <fichier>  JSON OAuth téléchargé depuis la console
                                    Google (défaut : <data>/youtus/client_secret.json)
    --cookies <fichier>             cookies.txt Netscape (repli sans OAuth)
    --token-file <fichier>          jeton OAuth (défaut : <data>/youtus/oauth.json)
    --db <fichier>                  base SQLite (défaut : <data>/youtus/youtus.db)
    -h, --help                      affiche cette aide
    -V, --version                   affiche la version

Le device flow s'affiche dans la fenêtre : aucun code à recopier en terminal.
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub token_path: PathBuf,
    pub credentials_path: PathBuf,
    pub cookies_path: Option<PathBuf>,
    pub db_path: PathBuf,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let data = data_dir();
        let mut config = Self {
            token_path: default_token_path(),
            credentials_path: data.join("client_secret.json"),
            cookies_path: None,
            db_path: data.join("youtus.db"),
        };

        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--token-file" => config.token_path = required(&mut args, "--token-file")?.into(),
                "--client-secret-file" | "--client-secret" => {
                    config.credentials_path = required(&mut args, "--client-secret-file")?.into()
                }
                "--cookies" => {
                    config.cookies_path = Some(required(&mut args, "--cookies")?.into())
                }
                "--db" => config.db_path = required(&mut args, "--db")?.into(),
                other => {
                    return Err(format!(
                        "option inconnue : {other}\n\n{USAGE}"
                    ))
                }
            }
        }

        Ok(config)
    }

    /// Résumé affiché dans la barre latérale : le chemin, jamais le contenu.
    pub fn summary(&self) -> String {
        if let Some(path) = &self.cookies_path {
            return format!("cookies : {}", path.display());
        }
        format!("jeton : {}", self.token_path.display())
    }
}

fn required(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, String> {
    match args.next() {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(format!("{flag} attend un chemin")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_chemins_par_defaut_sont_sous_youtus() {
        let config = Config::parse(Vec::new()).unwrap();
        assert!(config.token_path.ends_with("youtus/oauth.json"));
        assert!(config.credentials_path.ends_with("youtus/client_secret.json"));
        assert!(config.db_path.ends_with("youtus/youtus.db"));
        assert_eq!(config.cookies_path, None);
    }

    #[test]
    fn lit_les_options() {
        let config = Config::parse(
            [
                "--client-secret-file",
                "/tmp/client.json",
                "--cookies",
                "/tmp/cookies.txt",
                "--db",
                "/tmp/test.db",
            ]
            .map(str::to_owned),
        )
        .unwrap();

        assert_eq!(config.credentials_path, PathBuf::from("/tmp/client.json"));
        assert_eq!(config.cookies_path, Some(PathBuf::from("/tmp/cookies.txt")));
        assert_eq!(config.db_path, PathBuf::from("/tmp/test.db"));
        assert_eq!(config.summary(), "cookies : /tmp/cookies.txt");
    }

    #[test]
    fn refuse_une_option_inconnue_ou_sans_valeur() {
        assert!(Config::parse(["--inconnue".to_owned()]).is_err());
        assert!(Config::parse(["--db".to_owned()]).is_err());
    }
}
