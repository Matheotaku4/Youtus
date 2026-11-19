# S1 — Matrice de capacités réelles (OAuth device flow)

Statut : **à exécuter sur un compte réel**. Ce fichier est réécrit par la sonde :

```bash
cargo run --release --manifest-path spikes/s1-oauth/Cargo.toml -- \
  --client-secret-file ~/Téléchargements/client_secret_….json \
  --markdown docs/superpowers/spikes/2026-11-19-s1-capabilities.md \
  --json /tmp/s1-report.json
```

Le code du client OAuth se crée une seule fois : console Google Cloud → projet →
activer « YouTube Data API v3 » → écran de consentement externe en mode test avec
son compte en testeur → identifiant OAuth de type « TV et périphériques à entrée
limitée ». Le jeton est stocké dans `$XDG_DATA_HOME/youtus/oauth.json` (0600).

## Résultats

| Vérification | Résultat | Détail |
|---|---|---|
| _en attente d'exécution_ | — | — |

## Lecture

- Un échec sur `playbackTracking` invalide la progression OAuth : M3 bascule sur les
  cookies pour les beacons.
- Un échec sur `createCommentParams` confirme la limite connue du scope TV : M4
  exige des cookies pour publier, l'app doit griser le champ de commentaire.
- Le contexte client qui répond (WEB ou TVHTML5) est consigné : c'est celui que
  `youtus-yt` doit utiliser pour les requêtes authentifiées.
