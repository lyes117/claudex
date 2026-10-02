# Livraison et vérifications Claudex

Date : 3 octobre 2026. Ce rapport concerne un fork local fonctionnel, avec une compatibilité Claude partielle explicitement délimitée dans [CLAUDEX.md](CLAUDEX.md).

## Provenance et installation

- Dépôt : `C:\Users\lyesb\claudex`, branche `claudex/main`.
- Base officielle : `openai/codex`, tag `rust-v0.160.0`, commit `79b1b666f2e8551f8abbbca34957227f67f3f553`. Aucun fork distant publié.
- L'exécutable principal provient des sources modifiées du fork. Les auxiliaires inchangés proviennent du paquet officiel local 0.160.0 ; ce choix conserve le moteur Windows, le code-mode et les ressources existantes.
- Installation : `%LOCALAPPDATA%\Programs\Claudex\bin\claudex.exe`. Répertoire ajouté au PATH utilisateur, sans remplacement de Codex ou Claude.
- Reconstruction finale réussie avec MSVC, profil `dev-small` ; SHA-256 du binaire installé identique à celui compilé. Journal `.build-tools/build-shipping.log`.
- Authentification officielle partagée avec Codex : `Logged in using ChatGPT`. Le mode ChatGPT est forcé par défaut dans le fork. Aucune clé API distincte requise pour les appels vérifiés.
- Aucun dépôt existant migré. Aucun `CLAUDE.md`, `.claude`, `.mcp.json`, fichier de configuration global ou secret copié dans le fork.

## Vérifications fonctionnelles réelles

| Vérification | Résultat et preuve |
|---|---|
| Commande PowerShell et CMD | `claudex --version` : `claudex 0.160.0` ; résolution depuis le PATH utilisateur |
| Connexion | `claudex login status` : session ChatGPT reconnue |
| Inférence avec l'abonnement | Réponse réelle via `gpt-6.1-sol`, modèle disponible sur ce compte ; le modèle essayé auparavant, `gpt-5.4`, était refusé par le service |
| Instructions Claude du projet | Le modèle retourne une phrase configurée uniquement dans le `CLAUDE.md` de la fixture |
| Hook `SessionStart` | Processus Node effectivement exécuté ; événement enregistré ; `CLAUDE_PROJECT_DIR` égal au répertoire de session |
| Permission Claude `deny` | Une commande shell est réellement refusée par le garde avant dispatch |
| Hook `PreToolUse` | Une seconde commande est réellement refusée par la décision du hook |
| MCP stdio | Serveur Node démarré depuis `.mcp.json` ; appel effectif de son outil et retour attendu |
| Agent Markdown Claude | Exactement un sous-agent natif chargé depuis `.claude/agents/fixture-reviewer.md`, lancé puis attendu ; réponse issue de son rôle |
| Configuration habituelle | Inférence réelle avec la configuration utilisateur habituelle ; hooks désactivés pour ce seul contrôle |
| Terminal | Lancement en pseudo-terminal, en-tête Claudex et écran de connexion ChatGPT observés ; fermeture propre |
| Commande dans le terminal connecté | `/fixture-command "hello world" second` devient le contenu Markdown avec deux arguments correctement séparés ; réponse réelle `CLAUDEX_COMMAND_OK hello world second` |
| Workflow avec inférence | `claudex workflow` appelle réellement le fork final avec schéma JSON et obtient `{ "ok": true }` ; reprise validée sans lancement d'agent, checkpoint et date de modification inchangés |
| Workflow film existant | Le véritable `film.workflow.js` s'exécute avec `jobs: []`, sans agent ni production ; le contrat de chargement et les primitives sont exercés |

Les fixtures et sorties sont dans `.verification/`, ignoré par Git. `scripts/verify-runtime.mjs`, avec `--tools` ou `--agent`, reproduit les contrôles réels sans révéler les sorties ordinaires de Codex. Ce script utilise uniquement son projet de test, un bac à sable `read-only` et des commandes sans effet externe. Les hooks de cette fixture sont explicitement approuvés pour ce test ; le moteur normal conserve sa revue de confiance.

La production complète du film, les appels métier de ses agents et l'interface interactive exacte Claude ne sont pas attestés par le simple test à jobs vides. L'expansion des arguments possède également des contrôles Rust dédiés ; les snapshots couvrent l'identité et les écrans du terminal.

## Contrôles de code et état des suites

- Formatage requis `just fmt` exécuté. Les changements de formatage Bazel sans rapport avec le fork ont été retirés.
- `just fix` exécuté sur les crates touchées ; revue Rust puis revue JavaScript réalisées séquentiellement. Les problèmes relevés ont été corrigés : contexte des hooks, politiques natives, priorité des paramètres, restriction des métadonnées, mutation des checkpoints et verrous de workflow.
- `node scripts/workflows.test.mjs` passe : contrôle de flux, validation de sortie structurée, reprise sans réexécution, verrou et attente des agents avant libération. Ces tests utilisent des réponses de fixture et ne sont pas présentés comme des inférences réelles.
- Suite finale `codex-config` : **355 tests exécutés, 355 réussis**. Elle inclut le contrôle du périmètre des plugins lorsque la configuration utilisateur est ignorée, avec et sans racine Git. Journal `.build-tools/tests-config-shipping.log`.
- `just bazel-lock-update` a réussi après les changements de dépendances ; le verrou Bazel reste identique. La vérification du diff hors espaces de padding des snapshots passe.
- Première suite de sept paquets : **6 702 tests exécutés, 6 667 réussis, 31 échecs, 4 timeouts, 10 ignorés**. Journal `.build-tools/tests.log`. Certaines erreurs ont conduit aux correctifs ultérieurs ; ce résultat initial n'est pas un statut final vert.
- Suite compatibilité après ces correctifs : **671 tests exécutés, 668 réussis, 3 échecs**. Deux tests `codex-home` exigent un privilège Windows de création de liens symboliques absent (`1314`). Le troisième, `snapshot_for_config_merges_extension_host_and_legacy_plugin_roots`, obtient un catalogue différent dans cet environnement utilisateur ; son origine amont n'a pas été démontrée. Journal `.build-tools/tests-compat-final.log`.
- Suite core initiale : **2 544 exécutés, 2 532 réussis, 12 échecs, 3 ignorés**. Journal `.build-tools/tests-core.log`. Les sondes du système de fichiers, une valeur par défaut incorrecte et l'exclusion de configuration utilisateur ont été corrigées ensuite.
- Reprise ciblée core : **43 exécutés, 42 réussis, 1 échec** ; seul le test de lien symbolique échoue avec le privilège Windows absent. Les instructions, la confiance projet et la conservation des politiques d'exécution natives passent. Journal `.build-tools/tests-regressions.log`.

La suite complète du workspace n'a pas été déclarée verte ni exécutée : les suites de paquets conservent des échecs et des timeouts à analyser. Le fork est livré avec des vérifications fonctionnelles réelles ; la validation exhaustive multi-plateforme reste ouverte. Les journaux locaux détaillent les tentatives intermédiaires, sans être versionnés.

## Limites matérielles de compatibilité

1. Le TUI demeure celui de Codex, adapté pour Claudex. Les écrans et raccourcis Claude Code ne sont pas reproduits intégralement.
2. Les métadonnées de sécurité Claude non applicables au moteur natif entraînent le rejet de l'agent ou du skill concerné, plutôt qu'une exécution avec des permissions fictives. Les règles `ask` bloquent ; certains refus de chemins bloquent conservativement l'outil entier.
3. Les hooks gardent les payloads natifs Codex. Les hooks HTTP, prompt et agent, ainsi que les événements absents du moteur natif, ne sont pas pris en charge. Un script exigeant un payload Claude exact doit être adapté.
4. Les plugins locaux activés sont lus depuis leur registre et leurs répertoires standards. Les chemins personnalisés de manifeste, téléchargements et marketplace Claude ne sont pas implémentés.
5. Les workflows sont séquentiels, avec checkpoints et appels Codex réels. Il n'existe pas encore d'outil conversationnel natif `Workflow`, de workflows imbriqués, ni de pause ou reprise interactive équivalente à Claude. Le runtime JS exécute des programmes de confiance ; `node:vm` ne fournit pas une isolation de sécurité.
6. Le terminal local utilise le serveur embarqué. La vue `agents` multi-sessions, le daemon partagé et ses files locales ne sont pas activés dans cette installation. Cela évite de remplacer le daemon Codex existant.
7. SSE MCP, imports Markdown globaux `~`, activation dynamique exacte des règles par chemins et `${CLAUDE_PROJECT_DIR}` dans les corps de commandes ne sont pas couverts.

Les dépôts contenant ces cas restent directement ouvrables, mais chaque élément incompatible ne peut pas être annoncé comme fonctionnel. La matrice détaillée est dans [CLAUDEX.md](CLAUDEX.md).
