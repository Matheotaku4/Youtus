# S1 — Matrice de capacités réelles (OAuth device flow)

Mesuré le 2026-11-19 sur un compte réel (chaîne « matheotaku »), avec un client
OAuth « TV et périphériques à entrée limitée » créé pour l'occasion et le device
flow complet (code saisi sur `google.com/device`, refresh token stocké en 0600).

## Résultats

| Vérification | Résultat | Détail |
|---|---|---|
| Jeton → API publique Data v3 | ✅ | `channels?part=snippet&mine=true` → HTTP 200, chaîne « matheotaku » |
| Jeton → InnerTube (`account/account_menu`) | ❌ | HTTP 400 `INVALID_ARGUMENT` |
| Accueil personnalisé | ⏸ | non mesurable : aucun contexte authentifié |
| Historique | ⏸ | non mesurable |
| Like | ⏸ | non mesurable |
| `playbackTracking` | ⏸ | non mesurable |
| `createCommentParams` | ⏸ | non mesurable |

Profils InnerTube essayés avec le même Bearer, tous en 400 :

| Profil | clientVersion | En-tête | Résultat |
|---|---|---|---|
| WEB | 2.20261101.00.00 | 1 | 400 |
| TVHTML5 | 7.20260707.07.00 | 7 | 400 |
| TVHTML5 downgraded | 5.20260707 | 7 | 400 |
| TVHTML5 | 7.20250209.19.00 | 7 | 400 |

Contrôle : les mêmes requêtes **sans** `Authorization` renvoient 200 (WEB) et
401 « must be signed in » (TVHTML5) — le corps de requête et les contextes sont
donc valides. Seule l'ajout du Bearer déclenche le 400.

## Interprétation

Le jeton est **valide** (l'API Data v3 l'accepte et rend la chaîne du compte) mais
InnerTube le **refuse**. La conclusion qui tient avec tous les faits : les jetons
du device flow sont liés au client TV de YouTube, et un client OAuth créé par un
tiers — même du bon type — n'est pas reconnu par `youtubei/v1`. C'est cohérent
avec ce que font les clients qui fonctionnent (YouTube.js, Dmitry's WP YouTube,
aveuglément cités par MeeTube) : ils utilisent tous le **même couple public
d'identifiants TV**, pas un client personnel.

Conséquence pour Youtus : l'OAuth « apporte ton propre client » ne permet pas de
personnaliser `browse`/`like`/`history`. Il reste utile pour l'API Data v3
(chaînes, abonnements, playlists, notes), qui ne couvre ni l'historique de
lecture ni la progression.

## Décision ouverte

1. **Embarquer le couple TV public** (comme YouTube.js) : InnerTube authentifié
   immédiatement, mais dépendance à un identifiant Google rotatif et question de
   fond sur l'usage d'un client qui n'est pas le nôtre. À re-tester avant de
   s'engager.
2. **Cookies pour InnerTube + OAuth pour l'API Data v3** : pas de dépendance à un
   client tiers, mais l'utilisateur doit importer un `cookies.txt` pour l'accueil
   personnalisé, l'historique, le like et la progression.
