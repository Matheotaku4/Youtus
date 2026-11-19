mod innertube;
mod json_util;
mod oauth;
mod probe;

use std::path::PathBuf;

use anyhow::{bail, Result};
use innertube::{ClientKind, Innertube};
use serde_json::{json, Value};

const USAGE: &str = "\
s1-oauth — sonde headless des capacités OAuth de YouTube

USAGE:
    YOUTUS_OAUTH_CLIENT_ID=<id> s1-oauth [OPTIONS]

OPTIONS:
    --client-id <id>     client OAuth « TV et appareils à accès limité »
    --client <web|tv>    contexte InnerTube testé (défaut : web puis tv)
    --video <id>         vidéo de test (défaut : dQw4w9WgXcQ)
    --token-file <path>  jeton par défaut : $XDG_DATA_HOME/youtus/oauth.json
    --json <path>        écrit le rapport brut en JSON
    --markdown <path>    écrit la matrice en Markdown
    --skip-like          ne touche pas au like (laisse l'état d'origine)
    -h, --help           affiche cette aide
";

struct Args {
    client_id: Option<String>,
    client: Option<ClientKind>,
    video: String,
    token_file: PathBuf,
    json_out: Option<PathBuf>,
    markdown_out: Option<PathBuf>,
    skip_like: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        client_id: std::env::var("YOUTUS_OAUTH_CLIENT_ID").ok(),
        client: None,
        video: "dQw4w9WgXcQ".to_owned(),
        token_file: oauth::default_token_path(),
        json_out: None,
        markdown_out: None,
        skip_like: false,
    };

    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = || -> Result<String> {
            argv.next()
                .ok_or_else(|| anyhow::anyhow!("valeur manquante pour {flag}"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--client-id" => args.client_id = Some(value()?),
            "--client" => {
                args.client = Some(match value()?.as_str() {
                    "web" => ClientKind::Web,
                    "tv" => ClientKind::Tv,
                    other => bail!("--client attend `web` ou `tv`, reçu `{other}`"),
                })
            }
            "--video" => args.video = value()?,
            "--token-file" => args.token_file = PathBuf::from(value()?),
            "--json" => args.json_out = Some(PathBuf::from(value()?)),
            "--markdown" => args.markdown_out = Some(PathBuf::from(value()?)),
            "--skip-like" => args.skip_like = true,
            other => bail!("option inconnue : {other} (voir --help)"),
        }
    }

    Ok(args)
}

struct Row {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("\n✗ {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse_args()?;

    let Some(client_id) = args.client_id.clone() else {
        bail!(
            "YOUTUS_OAUTH_CLIENT_ID manquant.\n\
             Crée un client « TV et appareils à accès limité » sur \
             https://console.cloud.google.com/apis/credentials :\n\
             1. nouveau projet → API « YouTube Data API v3 » activée ;\n\
             2. écran de consentement OAuth (externe, mode test, ton compte en testeur) ;\n\
             3. créer un identifiant OAuth → type « TV et périphériques à entrée limitée » ;\n\
             4. relance : YOUTUS_OAUTH_CLIENT_ID=<id> cargo run --release"
        );
    };

    let stored = oauth::load_token(&args.token_file)
        .filter(|token| token.client_id == client_id);

    let access_token = match stored {
        Some(token) => {
            println!("• jeton trouvé dans {}, rafraîchissement…", args.token_file.display());
            oauth::refresh(&client_id, &token.refresh_token)?.access_token
        }
        None => {
            let code = oauth::request_device_code(&client_id)?;
            println!(
                "\n→ Ouvre {} et saisis le code : {}\n  (expire dans {} s)\n",
                code.verification_url, code.user_code, code.expires_in
            );
            let token = oauth::poll_token(&client_id, &code)?;
            let refresh_token = token
                .refresh_token
                .clone()
                .ok_or_else(|| anyhow::anyhow!("aucun refresh_token renvoyé"))?;
            oauth::save_token(
                &args.token_file,
                &oauth::StoredToken {
                    client_id: client_id.clone(),
                    refresh_token,
                },
            )?;
            println!("• jeton enregistré dans {}\n", args.token_file.display());
            token.access_token
        }
    };

    let candidates: Vec<ClientKind> = match args.client {
        Some(kind) => vec![kind],
        None => vec![ClientKind::Web, ClientKind::Tv],
    };

    let mut authenticated: Option<(Innertube, AccountInfo)> = None;
    let mut attempts = Vec::new();

    for kind in candidates {
        let mut yt = Innertube::new(kind, Some(access_token.clone()));
        match probe::probe_account(&mut yt) {
            Ok(account) if account.name.is_some() => {
                attempts.push((kind, format!("authentifié ({})", account.name.clone().unwrap())));
                authenticated = Some((yt, AccountInfo { name: account.name }));
                break;
            }
            Ok(_) => attempts.push((kind, "réponse sans compte (session anonyme ?)".to_owned())),
            Err(error) => attempts.push((kind, format!("{error:#}"))),
        }
    }

    let Some((mut yt, account)) = authenticated else {
        for (kind, detail) in &attempts {
            eprintln!("✗ {} : {detail}", kind.label());
        }
        bail!("aucun contexte InnerTube n'a accepté le jeton Bearer");
    };

    let client_label = yt.kind.label();
    println!("• client {client_label} authentifié : {}", account.name.clone().unwrap_or_default());
    println!("• vidéo de test : {}\n", args.video);

    let mut rows = Vec::new();
    rows.push(Row {
        name: "Authentification Bearer",
        ok: true,
        detail: format!("{client_label}, {}", account.name.clone().unwrap_or_default()),
    });

    match probe::probe_home(&mut yt) {
        Ok(home) => rows.push(Row {
            name: "Accueil personnalisé",
            ok: home.videos > 0,
            detail: format!(
                "{} vidéos{}",
                home.videos,
                if home.feed_nudge { " (feedNudge présent)" } else { "" }
            ),
        }),
        Err(error) => rows.push(Row {
            name: "Accueil personnalisé",
            ok: false,
            detail: format!("{error:#}"),
        }),
    }

    match probe::probe_history(&mut yt) {
        Ok(count) => rows.push(Row {
            name: "Historique",
            ok: count > 0,
            detail: format!("{count} entrées"),
        }),
        Err(error) => rows.push(Row {
            name: "Historique",
            ok: false,
            detail: format!("{error:#}"),
        }),
    }

    if args.skip_like {
        rows.push(Row {
            name: "Like",
            ok: false,
            detail: "non testé (--skip-like)".to_owned(),
        });
    } else {
        match probe::probe_like(&mut yt, &args.video) {
            Ok(like) => rows.push(Row {
                name: "Like",
                ok: like.liked,
                detail: format!(
                    "{}{}",
                    like.detail,
                    if like.restored { ", état d'origine restauré" } else { ", RESTAURATION ÉCHOUÉE" }
                ),
            }),
            Err(error) => rows.push(Row {
                name: "Like",
                ok: false,
                detail: format!("{error:#}"),
            }),
        }
    }

    let mut player_json = Value::Null;
    match probe::probe_player(&mut yt, &args.video) {
        Ok(player) => {
            player_json = json!({
                "status": player.status,
                "title": player.title,
                "watchtime": player.watchtime,
                "playback": player.playback,
                "atr": player.atr,
                "sts": player.sts,
            });
            rows.push(Row {
                name: "playbackTracking",
                ok: player.watchtime && player.playback,
                detail: format!(
                    "watchtime {}, playback {}, atr {} — {} ; playabilityStatus {}",
                    flag(player.watchtime),
                    flag(player.playback),
                    flag(player.atr),
                    player.detail,
                    player.status
                ),
            });
        }
        Err(error) => rows.push(Row {
            name: "playbackTracking",
            ok: false,
            detail: format!("{error:#}"),
        }),
    }

    match probe::probe_comments(&mut yt, &args.video) {
        Ok(comments) => rows.push(Row {
            name: "createCommentParams",
            ok: comments.params.is_some(),
            detail: format!(
                "continuation {}, paramètres {}, {} commentaires chargés",
                flag(comments.token_found),
                flag(comments.params.is_some()),
                comments.comments
            ),
        }),
        Err(error) => rows.push(Row {
            name: "createCommentParams",
            ok: false,
            detail: format!("{error:#}"),
        }),
    }

    println!("{:<26} {:<8} DÉTAIL", "VÉRIFICATION", "RÉSULTAT");
    println!("{}", "-".repeat(96));
    for row in &rows {
        println!("{:<26} {:<8} {}", row.name, flag(row.ok), row.detail);
    }

    let markdown = render_markdown(client_label, &args.video, &rows);
    if let Some(path) = &args.markdown_out {
        std::fs::write(path, &markdown)?;
        println!("\n• matrice écrite dans {}", path.display());
    }
    println!("\n{markdown}");

    if let Some(path) = &args.json_out {
        let report = json!({
            "generated_by": "s1-oauth",
            "client": client_label,
            "account": account.name,
            "video": args.video,
            "player": player_json,
            "rows": rows.iter().map(|row| json!({
                "name": row.name,
                "ok": row.ok,
                "detail": row.detail,
            })).collect::<Vec<_>>(),
        });
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        println!("• rapport JSON écrit dans {}", path.display());
    }

    Ok(())
}

struct AccountInfo {
    name: Option<String>,
}

fn flag(value: bool) -> &'static str {
    if value {
        "✅"
    } else {
        "❌"
    }
}

fn render_markdown(client: &str, video: &str, rows: &[Row]) -> String {
    let mut out = String::from("| Vérification | Résultat | Détail |\n|---|---|---|\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            row.name,
            flag(row.ok),
            row.detail.replace('|', "\\|")
        ));
    }
    out.push_str(&format!("\n_Client {client}, vidéo `{video}`._\n"));
    out
}
