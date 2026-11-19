# S1 — Matrice de capacités réelles (OAuth device flow)

Mesuré le 2026-11-19 sur un compte réel (chaîne « matheotaku »), device flow
complet (code saisi sur `google.com/device`, refresh token stocké en 0600), puis
sondé avec le client OAuth personnel **et** avec le couple TV public utilisé par
YouTube.js.

## Résultat mesuré (client TV public, profil TVHTML5 7.x)

| Vérification | Résultat | Détail |
|---|---|---|
| Authentification Bearer InnerTube | ✅ | session active : 20 tuiles d'accueil |
| Accueil personnalisé | ✅ | 20 `tileRenderer` avec jeton, **0** sans jeton |
| Historique | ✅ | 15 identifiants vidéo distincts |
| Like | ✅ | `likeStatus = LIKE` renvoyé, état d'origine restauré |
| Lire les commentaires | ✅ | `next` → continuation, 20 `commentThreadRenderer` |
| `playbackTracking` | ⏸ non mesurable ici | `/player` → `UNPLAYABLE` « La page doit être actualisée », **même en anonyme WEB** : IP de datacenter bloquée |
| `createCommentParams` | ❌ | absent en TV et en WEB anonyme (publication réservée à une session WEB connectée) |
| API Data v3 | ⏸ | 403 sur le projet 861556708454 (API non activée) ; ✅ 200 sur le client personnel |

## Ce que S1 a démenti

1. **« Le jeton OAuth maison est refusé par InnerTube »** — faux. Le 400 venait de
   `account/account_menu`, endpoint qui renvoie 400 *même avec le jeton TV public
   de Google*. `browse`, `like` et `next` acceptent le bearer sans broncher. La
   sonde prenait un mauvais portail d'authentification ; elle utilise désormais
   l'accueil personnalisé (20 tuiles connecté vs 0 anonyme).
2. **« Il faut forcément les identifiants TV publics »** — non démontré. Le
   couple public fonctionne (mesures ci-dessus), mais le jeton personnel n'a
   jamais été testé sur `browse` : c'est la seule chose qui reste à mesurer.

## Ce que S1 a confirmé

- Le jeton d'un client OAuth personnel est **bien lié à un type de client** : le
  même bearer renvoie 400 sur le contexte WEB et 200 sur TVHTML5.
- `playbackTracking` n'apparaît que dans une réponse `/player` jouable ; sans
  flux, aucun beacon de progression n'est exposé.
- La publication de commentaires n'expose `createCommentParams` que pour une
  session connectée du bon type — le client TV ne le fournit pas.

## Méthode

- `spikes/s1-oauth` : `probe_auth` (accueil + historique), `probe_home`,
  `probe_history` (identifiants vidéo uniques), `probe_like`, `probe_player`
  (sts TV lu dans `tv-player-ias.js`), `probe_comments`.
- Profils essayés dans l'ordre, arrêt au premier qui rend une session active.
- Aucune valeur secrète écrite dans le dépôt : le couple TV public est récupéré
  par script depuis la documentation publique du projet, et stocké hors dépôt.

## Limites de la mesure

- `/player` est `UNPLAYABLE` depuis la sandbox pour **toutes** les combinaisons,
  y compris anonyme : la sonde doit être relancée depuis la machine de
  l'utilisateur pour trancher `playbackTracking`.
- Le compte n'a probablement jamais noté la vidéo témoin : le like a été posé
  puis retiré (`like/removelike`), l'état d'origine est restauré.

## Décision

La couche `AuthStrategy` reste **obligatoire** : `Anonymous`, `Bearer` (client
personnel ou TV public), `Sapisid` (cookies). Le player et la publication de
commentaires ont besoin d'une session WEB, pas du client TV — c'est une
contrainte d'architecture, pas un détail de configuration.
