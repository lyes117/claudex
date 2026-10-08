# Claudex — Pont local ZCode (révision de l'architecture Z.ai)

Écrit le 5 octobre 2026, à la demande du porteur, en remplacement de la voie
OAuth du plan Z01–Z20. Décision : **pas d'OAuth** — Claudex route l'inférence
GLM vers l'installation ZCode locale de la machine, qui porte le compte
Z.ai du porteur et son quota d'abonnement. Objectifs inchangés : 0 € de
consommation API, quota d'abonnement uniquement, GLM 5.3 dans le CLI, un
orchestrateur qui déclenche les agents, et une intégration A2A.

## 1. Faits mesurés le 5 octobre 2026 (sources locales, lecture seule)

| Fait | Preuve |
|---|---|
| L'app ZCode desktop tourne (8 processus) ; son processus Node (utility NodeService) écoute un port local éphémère (`127.0.0.1:50254` au moment de la mesure) | `tasklist` + `netstat -ano`, PID 16076 |
| Ce port est le serveur média interne (ressource/cloud content), pas une API externe : `listen(0,"127.0.0.1")` + `unref()`, route `/__zcode_media/` | bundle `out/host/index.js` (extrait 1,5 Mo dans `.build-tools/zai-continuation/bundle-analysis/`) |
| Les sessions CLI lancées par le desktop reçoivent `ZCODE_BASE_URL=https://zcode.z.ai` — le backend est DISTANT, l'app n'est pas un proxy d'inférence local | noms de variables d'env des sessions CLI (valeurs non lues sauf host:port de BASE_URL) |
| **Le CLI officiel expose `zcode app-server` (« Run the ZCode Protocol stdio app server »)**, lancé par le desktop avec `spawnArgs:["app-server","--stdio"]` | `zcode.cjs --help` (v0.16.9) + chaîne `spawnArgs` dans le bundle |
| La surface du protocole couvre exactement le besoin du pont : `session/create · send · events · subscribe · setModel · setMode · usage · subagents · fork · resume · compact · stop · list · goal` | méthodes JSON-RPC du bundle `zcode.cjs` |
| Le CLI est autonome : login propre (`zcode login/logout`), credentials chiffrés par device que **lui seul** lit (`~/.zcode`), indépendant du desktop UI | `--help` + doc QA officielle (credentials « encrypted per device ») |
| A2A est en v1.0 (2026) sous Linux Foundation : JSON-RPC 2.0, Agent Card à `/.well-known/agent-card.json`, cartes signées | recherche web (agent2agent, helloskip 13/09/2026) |

**Conclusion** : le point d'intégration propre n'est ni le port interne de
l'app, ni un nouveau sign-in — c'est le **processus `zcode app-server --stdio`**,
surface officielle locale du protocole ZCode, que Claudex spawn et pilote.

## 2. Architecture

```
┌────────────────────────────── Claudex (Rust) ──────────────────────────────┐
│  TUI (identique Claude Code)                                               │
│  Orchestrateur = workflow V8 natif + sous-agents Codex                     │
│      │                                  │                                  │
│      │ Provider "zcode-bridge"         │ Provider A2A v1.0                │
│      │ (nouveau)                       │ (client + serveur)               │
│  ┌───▼──────────────┐            ┌──────▼─────────┐   ┌──────────────────┐ │
│  │ Pont stdio :     │            │ A2A : carte    │   │ Agents distants  │ │
│  │ spawn zcode      │            │ signée + client│◄──┤ (toute tech,     │ │
│  │ app-server       │            │ JSON-RPC       │   │ carte publique)  │ │
│  └───┬──────────────┘            └────────────────┘   └──────────────────┘ │
└──────┼─────────────────────────────────────────────────────────────────────┘
       │ stdio JSON-RPC (ZCode Protocol)
┌──────▼─────────────────────────────────────────────────────────────────────┐
│ zcode app-server (install local ZCode, v0.16.9)                            │
│  • lit LUI-MÊME ses credentials chiffrés (~/.zcode) — Claudex n'y touche pas│
│  • session/create → setModel GLM-5.3 → setMode → send → events (stream)    │
│  • session/usage = consommation visible ; quota = abonnement, 0 € API      │
└─────────────────────────────────────────────────────────────────────────────┘
```

Rôles :
1. **Pont** (`zcode-bridge`) : un module Rust qui spawn `node <install>/resources/glm/zcode.cjs app-server --stdio` (chemin découvert via `ZCODE_WINDOWS_APP_INSTALL_DIR`/registre standard), maintient le processus chaud, parlé en JSON-RPC. Le credential d'inférence reste **dans** le processus ZCode — Claudex ne stocke AUCUN secret Z.ai (Z05 se réduit à « aucun secret », juste un état de session local non sensible).
2. **Provider d'enfant** : l'orchestrateur natif (V8, sous-agents) gagne un type d'agent `glm` routé par le pont — un enfant GLM garde permissions/sandbox du parent, les événements traduits au format des items Claudex, le ledger raisonnement (Z09) raccordé aux événements du protocole.
3. **Quota** : `session/usage` du protocole comme source de vérité locale ; réservation locale pessimiste avant admission (Z07), refus propre si usage inconnu. Jamais d'endpoint payant.
4. **Orchestrateur** : le workflow V8 existant déclenche des agents Codex (parent ChatGPT) **et** des agents GLM (pont), parallélisme par vaves, permissions vérifiées avant admission — inchangé, un provider de plus.
5. **A2A v1.0** : ① Claudex **sert** une Agent Card (`/.well-known/agent-card.json`) qui expose l'orchestrateur (skills = workflows natifs) ; ② Claudex **appelle** des agents externes par JSON-RPC (`message/send`, `message/stream`, `tasks/get`, `tasks/cancel`), cartes vérifiées (signatures v1.0), aucun effet externe sans permission du parent.

## 3. Ce que ça change au plan Z01–Z20

| Étape | Avant | Après le pivot |
|---|---|---|
| Z02 | bloqué externe (OAuth tiers) | **clos** : plus de contrat OAuth à obtenir ; le pont est local |
| Z03 | 5 types de credentials Z.ai | réduit : **aucun credential Z.ai chez Claudex** ; un seul état « session pont » non secret |
| Z04 | sign-in dédié + machine d'état | remplacé par **santé du pont** : app/CLI présent ? login fait ? sinon message clair (l'utilisateur se connecte via ZCode, comme aujourd'hui) |
| Z05 | store indépendant | **aucun secret stocké** — supprime la tranche (le store Codex existant reste) |
| Z06 | entitlement + résolution de clé | remplacé par `session/usage` + échec explicite si non connecté |
| Z07 | quota crédits modélisé | réservation pessimiste locale + lecture `usage` ; modèles GLM-5.3 / 5.3-Flash via `session/setModel` |
| Z08 | transport Chat Completions direct | **dormant** (gardé pour plus tard) ; l'inférence passe par le pont |
| Z09 | ledger reasoning | inchangé, raccordé aux événements du pont |
| Z10 | runtime d'enfant | le pont EST le runtime d'enfant GLM ; traduction d'événements à faire |
| Z11–Z14 | admission/capacités/routage/fallback | inchangés dans l'esprit ; fallback = parent Codex (jamais d'API payante) |
| Nouveau | — | **PA1** pont stdio (handshake, framing, cycle de vie) · **PA2** provider GLM dans l'orchestrateur · **PA3** A2A v1.0 serveur+client · **PA4** quota/usage et modes |

## 4. Inconnues à lever (première expérimentation, 0 quota d'inférence)

1. **Framing stdio exact** : JSON par ligne (attendu, lignée Claude Code) vs autre — un aller-retour `initialize` suffit.
2. **Forme du handshake** : nom/version du protocole, capabilities, réponse d'erreur.
3. **Formes de payload** `session/*` (create/send/events/usage) — à capturer depuis le client réel (le TUI du CLI) ou un fixture local.
4. **`session/subagents`** : sémantique exacte (liste ? orchestration native ZCode ?) — décide si l'orchestrateur Claudex pilote les sous-agents ZCode ou les siens.
5. **Multi-instances** : un pont par session Claudex ou un singleton partagé — verrou/état par cwd.

Méthode : fixture stdio locale (spawn + initialize + session/create + stop), **aucune inférence**, puis mocks Rust complets, puis branchement réel après revue. Les essais d'inférence réels (payés en quota d'abonnement) ne se font qu'avec l'accord explicite du porteur.

## 5. Contraintes préservées

- **0 € API** : le pont n'appelle que le backend de l'abonnement via le CLI officiel ; aucune clé, aucun endpoint `/api/paas/v4`, aucun fallback payant — le fallback d'inférence reste le parent Codex.
- **Aucun credential lu/émigré** : Claudex ne lit jamais `~/.zcode` ni les stores ZCode ; le secret reste dans le processus enfant.
- **Aucune usurpation d'identité** : pas de client ID ZCode emprunté — le CLI officiel parle en son nom propre, connecté au compte du porteur.
- **Preservation** : l'app ZCode n'est jamais lancée/tuée/modifiée par Claudex ; si le login manque, message et refus propre.
- Les limites de la méthode : le protocole `app-server` est une surface locale non documentée publiquement — son évolution entre versions ZCode sera suivie par la dérive de version (le pont refuse une version inconnue au handshake au lieu de deviner).
