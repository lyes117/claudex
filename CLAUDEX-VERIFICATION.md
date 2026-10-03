# Livraison et vérifications Claudex

Date : 3 octobre 2026. Ce rapport concerne un fork local fonctionnel, avec une compatibilité Claude partielle explicitement délimitée dans [CLAUDEX.md](CLAUDEX.md).

## Provenance et installation

- Dépôt : `C:\Users\lyesb\claudex`, branche `claudex/main`.
- Base officielle : `openai/codex`, tag `rust-v0.160.0`, commit `a956835d020762cb2b570053af06f643a11c0ecc`. Aucun fork distant publié.
- L'exécutable principal, CodeMode et les deux helpers Windows sont désormais compilés depuis les sources du fork. Les ressources voix et ripgrep restent issues du paquet officiel local 0.160.0. La reconstruction utilise la paire V8 sandbox officielle OpenAI et sa vérification de manifeste épinglée, sans désactiver le sandbox V8.
- Installation : `%LOCALAPPDATA%\Programs\Claudex\bin\claudex.exe`. Répertoire ajouté au PATH utilisateur, sans remplacement de Codex ou Claude.
- Reconstruction initiale réussie avec MSVC, profil `dev-small` (`.build-tools/build-shipping.log`), puis reconstruction TUI/workflows réussie (`.build-tools/build-cycle6.log`). SHA-256 du binaire installé identique à celui compilé à chaque installation vérifiée.
- Authentification officielle partagée avec Codex : `Logged in using ChatGPT`. Le mode ChatGPT est forcé par défaut dans le fork. Aucune clé API distincte requise pour les appels vérifiés.
- Aucun dépôt existant migré. Aucun `CLAUDE.md`, `.claude`, `.mcp.json`, fichier de configuration global ou secret copié dans le fork.

### Cycle héritage à chaud et helpers compilés

`scripts/build-claudex.ps1 -IncludeTestFixtures` réussit en **6 min 25 s** : `.build-tools/build-hot-policy-native-all-cycle1.log`. Il compile le CLI, le host CodeMode, les deux helpers Windows et le serveur MCP de fixture. Les probes et Cargo utilisent le même cwd ; le dossier de sortie est fixé. Le contrôle réel confirme qu'un `CARGO_TARGET_DIR` différent est supplanté et que le cwd ainsi que les variables V8/repo du caller sont restaurés. Les refus de cible ARM et de cible dans un CARGO_HOME relatif sont aussi vérifiés, sans lancement de build incompatible.

Installation réussie : `.build-tools/install-hot-policy-native-all-cycle1.log`. Les quatre SHA-256 source/installé correspondent. CLI : `DB5C77407419CDB89C05A680B4EDB4BF2E7E79F48B6322AB0EFAA44E04106E44`. Host CodeMode : `A6785BEA38024D4EDE31A9F31844421135A383BC5466EC0F894FCE94051EF98D`. PowerShell sans profil et CMD, avec le PATH utilisateur/machine, lancent `claudex 0.160.0` et reconnaissent `Logged in using ChatGPT`.

Les sources du runtime V8 et du host passent **151/151** tests (`tests-source-v8-runtime-host-cycle1.log`). L'API de plafonds passe **12/12**, dont 1600 compositions. Les vérifications natives d'héritage à chaud sont détaillées dans [CLAUDEX-TOOL-POLICY-DESIGN.md](CLAUDEX-TOOL-POLICY-DESIGN.md). Ce cycle n'active pas les contraintes des profils Claude : la restauration durable est encore ouverte. La suite core complète reste non verte ; les tests qui provisionnent des comptes sandbox Windows ne sont pas exécutés dans les reprises ciblées de ce cycle.

Avec cette nouvelle installation et son helper CodeMode compilé, `scripts/file-tools-live.test.mjs` passe réellement via ChatGPT : `Read`, `Grep`, `Glob`, modes direct/CodeMode et restauration des cartes natives. Preuves : `.build-tools/file-tools-live-source-host-hot-policy-cycle1.log` et `.build-tools/live-file-tools/run-MkFkG3/verified.json`. Le catalogue public de fixture sélectionne les deux modes ; aucune authentification n'est copiée. Cette inférence ne vérifie pas un profil Claude contraint ni une reprise durable de plafond.

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
| Contrôle réel des workflows | `scripts/workflow-live.test.mjs` : deux inférences structurées réelles, pause après l'agent actif, reprise, refus de `--run-id` doublé, deux résultats effectivement rejoués sans nouvelle inférence |
| Arrêt réel | Processus enfant Codex démarré puis arrêté ; PID disparu, premier agent arrêté, second agent conservé en attente sans démarrage, verrou libéré. Cela ne prouve pas qu'un petit-enfant shell avait démarré avant l'arrêt |
| Nouveaux panneaux installés | Pseudo-terminal : aide recherchable, catalogue des rôles, tâches de session et panneau workflows ouverts ; ce dernier affiche les deux exécutions réelles et leur état |

Les fixtures et sorties sont dans `.verification/`, ignoré par Git. `scripts/verify-runtime.mjs`, avec `--tools` ou `--agent`, reproduit les contrôles réels sans révéler les sorties ordinaires de Codex. Ce script utilise uniquement son projet de test, un bac à sable `read-only` et des commandes sans effet externe. Les hooks de cette fixture sont explicitement approuvés pour ce test ; le moteur normal conserve sa revue de confiance.

Le contrôle workflow supplémentaire est volontairement explicite : définir `CLAUDEX_LIVE_BIN` vers le fork installé puis lancer `node scripts/workflow-live.test.mjs`. Il utilise le compte ChatGPT déjà connecté ; ses artefacts sont dans `.build-tools/live-workflow-checks/2f149a44-ae16-4cb5-81e3-4ecbe8a6ccf3`. Résultat : réussite ; journal `.build-tools/workflow-live-cycle6.log`. Il ne lance pas le workflow marketing de production.

La production complète du film, les appels métier de ses agents et l'interface interactive exacte Claude ne sont pas attestés par le simple test à jobs vides. L'expansion des arguments possède également des contrôles Rust dédiés ; les snapshots couvrent l'identité et les écrans du terminal.

## Contrôles de code et état des suites

- Cycle outils natifs : **12/12** tests ciblés réussis (`.build-tools/tests-file-tools-integration-cycle7.log`), incluant fichiers réels, CodeMode via l'auxiliaire officiel, refus avant exécution, priorité client et restauration après redémarrage dans les deux formats d'historique. Ces tests utilisent un modèle de fixture, pas une inférence ChatGPT. Suite élargie : **992 exécutés, 990 réussis, 2 échecs** ; les deux échecs étaient un nouveau snapshot manquant et le schéma interne précompilé obsolète. Après correction, reprise **2/2** réussie (`.build-tools/tests-file-tools-broad-cycle2.log`). Cela ne constitue pas une nouvelle exécution verte de toutes les suites complètes.
- Nouveau snapshot Read/Grep/Glob examiné aux largeurs 26 et 80, en cours et restauré ; sortie intégrale conservée dans l'item, aperçu visuel borné. `just bazel-lock-update` réussi, verrou inchangé. Les quatre tests Windows du lanceur de vérification passent : capture bornée, parsing sans divulgation, timeout et arrêt borné. Revues adversariales Rust et JavaScript séquentielles réalisées.
- Revue finale du budget : erreur backend longue et nom arbitraire dans un payload incompatible reproduits avant correction, puis **11/11** tests du paquet réussis (`.build-tools/tests-file-tools-error-bound-green.log`). JSON réussi borné après échappement ; erreur bornée en octets UTF-8 avant sérialisation de l'enveloppe, `Fatal` conservé avec message constant. Relecture ciblée sans défaut P0/P1. Premier build CLI réussi, mais antérieur à ce correctif ; il n'a pas été installé.
- Reconstruction corrigée réussie (`.build-tools/build-file-tools-cycle2.log`), formatage et Clippy ciblé réussis. Binaire installé et hash identique : `7287A83CB118F83498730350B5325B6666C54EB9C97CBBFD7261988CAA6BE027`. PowerShell et CMD, PATH utilisateur frais : version et connexion ChatGPT réussies. Premier test réel du nouveau cycle **échoue sur son assertion de mode**, après appel et restauration des trois cartes : les métadonnées officielles de `gpt-6.1-sol` imposent `code_mode_only`, prioritaire sur les feature flags. La fixture est corrigée pour sélectionner les deux modes par `model_catalog_json` local, sans modifier le cache officiel ni la configuration utilisateur. Reprise **réussie** : les trois outils réellement exécutés et leurs cartes restaurées dans les deux modes (`.build-tools/file-tools-live-cycle3.log`, `.build-tools/live-file-tools/run-jkXQEh/verified.json`). Cette preuve contrôlée ne signifie pas que les feature flags seuls changent le mode officiel du modèle.
- Terminal installé en pseudo-terminal Windows : appel des trois outils sur le fichier synthétique, réponse `FILE_TOOLS_OK`, cartes Glob, Read et Grep effectivement observées dans la vue transcript (`Ctrl+T`). Configuration globale habituelle chargée ; hooks désactivés uniquement pour cet essai. Les avertissements de profils Claude incompatibles et de budget de skills restent visibles : cette session ne prouve pas le chargement de tous les agents ou skills.
- Cycle publication après hooks et interruption : reproduction rouge du dispatch abandonné après stockage, puis **19/19** contrôles core, **20/20** contrôles de publication native et **5/5** fonctions d'intégration app-server réussis. Les scénarios couvrent hooks exécutés, refus, restauration dans les deux formats et cellule CodeMode survivant à une fin normale de tour. Les tests utilisent un modèle de fixture et l'auxiliaire CodeMode officiel. La publication indépendante du callback et la garde synchrone ferment les chemins testés ; un arrêt brutal du processus/runtime ne garantit pas la persistance. Les complétions tardives mettent à jour les cellules TUI retenues, jusqu'à 256 ; le scrollback physique imprimé n'est pas garanti actualisé.
- Ce cycle est reconstruit et installé (`build-file-publication-cycle1.log`, `install-file-publication-cycle1.log`) : hash source/installation identique `2884C88FDA9C4DDFFEFF3573AD85B8E96189D418183332B3DA40AA9803F7371F`. PowerShell et CMD avec PATH frais reconnaissent version et connexion ChatGPT. Reprise du contrôle réel **réussie** dans les deux modes (`file-publication-live-cycle1.log`, `live-file-tools/run-8UU1Nq/verified.json`). TUI installée : trois cartes observées et `FILE_TOOLS_OK`, fermeture propre, indication `claudex resume 01a10048-859e-72c1-9237-49969abc0857`. **32/32** contrôles des indications de reprise passent ; formatage et Clippy ciblé réussis (`tests-resume-hints-cycle1.log`, `fix-resume-hints-cycle1.log`). Les événements CLI JSON des outils natifs restent à implémenter ; aucune nouvelle suite complète verte n'est revendiquée.
- Formatage requis `just fmt` exécuté. Les changements de formatage Bazel sans rapport avec le fork ont été retirés.
- `just fix` exécuté sur les crates touchées ; revue Rust puis revue JavaScript réalisées séquentiellement. Les problèmes relevés ont été corrigés : contexte des hooks, politiques natives, priorité des paramètres, restriction des métadonnées, mutation des checkpoints et verrous de workflow.
- `node scripts/workflows.test.mjs` passe : contrôle de flux, validation de sortie structurée, reprise sans réexécution, verrou et attente des agents avant libération. Ces tests utilisent des réponses de fixture et ne sont pas présentés comme des inférences réelles.
- `node --test scripts/workflow-control.test.mjs` : **10/10 réussis** ; modification de script, invalidation du suffixe, pause, reprise, arrêt, échec retournant `null`, horloge implicite refusée, erreurs de checkpoint propagées et phase capturée à la déclaration des agents en attente. Le nouveau cas de phase a d'abord échoué sur le défaut, puis passe après correction (`.build-tools/tests-workflow-phase-red.log`, `.build-tools/tests-workflow-phase-green.log`). La revue a trouvé une erreur de stockage interceptable par le script ; sa reproduction échoue avant correction, puis passe avec l'erreur maintenue fatale (`.build-tools/tests-workflow-pending-storage-red.log`). Le contrat général passe aussi (`.build-tools/tests-workflow-phase-contract.log`).
- Cycle TUI/workflows : revues adversariales Rust et JavaScript séquentielles, correctifs puis `just fix -p codex-tui -p codex-cli --profile dev-small` réussi (`.build-tools/fix-cycle6.log`). Dernière suite TUI complète : **5 549 exécutés, 5 536 réussis, 11 échecs, 2 timeouts, 8 ignorés** (`.build-tools/tests-tui-cycle4.log`). Reprises ciblées : **64/66**, puis **12/12** après les deux corrections restantes (`.build-tools/tests-tui-targeted-cycle6.log`). Les snapshots intentionnels ont été examinés ; les snapshots de couleurs ANSI et de locale sans rapport n'ont pas été remplacés pour masquer les différences d'environnement. Les deux timeouts des tests de worktree restent à analyser. Ce n'est pas un statut vert de l'ensemble du TUI.
- Suite finale `codex-config` : **355 tests exécutés, 355 réussis**. Elle inclut le contrôle du périmètre des plugins lorsque la configuration utilisateur est ignorée, avec et sans racine Git. Journal `.build-tools/tests-config-shipping.log`.
- `just bazel-lock-update` a réussi après les changements de dépendances ; le verrou Bazel reste identique. La vérification du diff hors espaces de padding des snapshots passe.
- Première suite de sept paquets : **6 702 tests exécutés, 6 667 réussis, 31 échecs, 4 timeouts, 10 ignorés**. Journal `.build-tools/tests.log`. Certaines erreurs ont conduit aux correctifs ultérieurs ; ce résultat initial n'est pas un statut final vert.
- Suite compatibilité après ces correctifs : **671 tests exécutés, 668 réussis, 3 échecs**. Deux tests `codex-home` exigent un privilège Windows de création de liens symboliques absent (`1314`). Le troisième, `snapshot_for_config_merges_extension_host_and_legacy_plugin_roots`, obtient un catalogue différent dans cet environnement utilisateur ; son origine amont n'a pas été démontrée. Journal `.build-tools/tests-compat-final.log`.
- Suite core initiale : **2 544 exécutés, 2 532 réussis, 12 échecs, 3 ignorés**. Journal `.build-tools/tests-core.log`. Les sondes du système de fichiers, une valeur par défaut incorrecte et l'exclusion de configuration utilisateur ont été corrigées ensuite.
- Reprise ciblée core : **43 exécutés, 42 réussis, 1 échec** ; seul le test de lien symbolique échoue avec le privilège Windows absent. Les instructions, la confiance projet et la conservation des politiques d'exécution natives passent. Journal `.build-tools/tests-regressions.log`.

La suite complète du workspace n'a pas été déclarée verte ni exécutée : les suites de paquets conservent des échecs et des timeouts à analyser. Le fork est livré avec des vérifications fonctionnelles réelles ; la validation exhaustive multi-plateforme reste ouverte. Les journaux locaux détaillent les tentatives intermédiaires, sans être versionnés.

## Conservation des paramètres CLI entre les sous-commandes

Le parser du fork collecte désormais les occurrences de `-c` et `--config` à chaque niveau de la commande, avec Clap, puis les transmet une seule fois dans leur ordre initial. Les arguments littéraux après `--` restent des arguments du prompt ou du serveur MCP. Les options générées au niveau général sont insérées avant les paramètres descendants, afin qu'un `sandbox_mode="read-only"` explicite après `resume` conserve sa priorité face à `--approve-for-me`.

Deux tests ont d'abord reproduit la perte d'options ; deux autres ont reproduit l'inversion de priorité relevée par la revue adversariale. Après correction, les **314 tests du binaire CLI passent**, avec un test ignoré (`tests-global-config-args-cli-bin-cycle3.log`). La suite du paquet CLI exécutée avant le second correctif donne **460 réussites, huit échecs, deux expirations et deux tests ignorés** (`tests-global-config-args-cli-package-cycle1.log`). Elle n'est pas verte : les chemins du daemon, du tableau d'agents, du worktree, de l'exec-server et deux fixtures distantes restent à analyser. Le seul échec de l'aide du binaire a été corrigé pour le nom `claudex`, puis revérifié dans les 314 tests. Les autres options globales à valeurs multiples et les parsers autonomes restent hors de ce correctif.

Formatage et Clippy ciblé réussis (`fmt-global-config-args-cycle2.log`, `fix-global-config-args-cycle2.log`), puis construction native et installation réussies (`build-global-config-args-native-cycle1.log`, `install-global-config-args-native-cycle1.log`). Le hash du binaire source et installé correspond : **5FCC818C83495372FDF925834995E6006A2EA7E1417434C854CBE15A8DCC8C24**. PowerShell sans profil et CMD avec PATH frais reconnaissent la commande et la connexion ChatGPT ; la complétion PowerShell enregistre bien `claudex`.

Le contrôle réel ChatGPT passe avec le catalogue au niveau général et les autres paramètres après `exec` : trois outils natifs exécutés, en direct puis en CodeMode, avec restauration de leurs cartes (`file-tools-live-mixed-config-cycle1.log`, `live-file-tools/run-E9sC8f/verified.json`). Le helper source CodeMode conserve le hash **A6785BEA38024D4EDE31A9F31844421135A383BC5466EC0F894FCE94051EF98D**, identique à l'installation. Le test vérifie explicitement que le modèle officiel utilise CodeMode par défaut, puis contrôle les identifiants des appels pour détecter la perte du catalogue direct. Il prouve cette transmission du catalogue ; il ne prouve pas l'effet individuel de chaque autre paramètre, ni une reprise CLI à trois niveaux sous ChatGPT.

## Plafond sans outils et premier transport Workflow natif

Les commits e581c93/129300d ajoutent le plafond tools.enabled, les helpers
temporaires et les preuves adverses ; 1845d13 transporte le schema du premier
tour enfant, et f8d7543 stabilise les fixtures terminales Windows. Formatage et
Clippy core/config/TUI reussis. Sept controles natifs du plafond, 47 controles
TUI cibles et quatre controles du schema enfant passent. La suite TUI complete
donne 5556/5557 reussites, 301 leaky et huit ignores ; son dernier echec de fixture
follow est corrige, revu puis repris seul avec succes. La suite core/config
complete reste non verte : 4587/4609, 21 echecs et une expiration.

Build et installation reussis, revision source f8d7543. Quatre executables source
et installes identiques ; hash principal :
**4D8A3292F84BBE4B655801348F150DAB60B2719371E461121E61FA3503BFCF62**.
PowerShell/CMD avec PATH frais reconnaissent claudex et l'authentification ChatGPT.
Inference reelle sans outils reussie avec le catalogue officiel sans override
(`no-tools-live-cycle1.log`, `live-no-tools/run-OsLRTt/verified.json`). Le test
verifie les items CLI termines et le marqueur textuel ; il ne prouve pas chaque
absence d'operation filesystem. Read/Grep/Glob restent fonctionnels en direct et
CodeMode avec leurs cartes restaurees (`file-tools-live-after-no-tools-cycle1.log`,
`live-file-tools/run-Z1AZUu/verified.json`, catalogue de fixture explicite).
Les journaux et receipts sont dans `.build-tools/`. Aucun outil Workflow natif
n'est active par le seul transport de schema ; ses prochaines tranches sont
decrites dans CLAUDEX-NATIVE-WORKFLOW-DESIGN.md.

## Limites matérielles de compatibilité

1. Le TUI demeure celui de Codex, adapté pour Claudex. Les écrans et raccourcis Claude Code ne sont pas reproduits intégralement.
2. Les métadonnées de sécurité Claude non applicables au moteur natif entraînent le rejet de l'agent ou du skill concerné, plutôt qu'une exécution avec des permissions fictives. Les règles `ask` bloquent ; certains refus de chemins bloquent conservativement l'outil entier.
3. Les hooks gardent les payloads natifs Codex. Les hooks HTTP, prompt et agent, ainsi que les événements absents du moteur natif, ne sont pas pris en charge. Un script exigeant un payload Claude exact doit être adapté.
4. Les plugins locaux activés sont lus depuis leur registre et leurs répertoires standards. Les chemins personnalisés de manifeste, téléchargements et marketplace Claude ne sont pas implémentés.
5. Les workflows sont séquentiels, avec checkpoints, pause/reprise/arrêt depuis la CLI ou le panneau et appels Codex réels. L'outil conversationnel natif `Workflow`, les workflows imbriqués, la relance individuelle, la concurrence et la validation complète des schémas Claude restent ouverts. Le runtime JS exécute des programmes de confiance ; `node:vm` ne fournit pas une isolation de sécurité.
6. Le tableau `/agent-center` et `claudex agents` fonctionnent maintenant sur le serveur embarqué installé. Quatre fixtures natives couvrent la découverte, deux sessions vides avec brouillons, archive/delete et la lecture d'un writer externe (deux serveurs dans le même processus). Le terminal réel confirme tableau, deux créations, ouverture/retour et fermeture normale. Le contrôle live entre processus et les tâches détachables restent non prouvés. Voir CLAUDEX-AGENTS-NAVIGATION-DESIGN.md pour les suites non vertes et les journaux.
7. SSE MCP, imports Markdown globaux `~`, activation dynamique exacte des règles par chemins et `${CLAUDE_PROJECT_DIR}` dans les corps de commandes ne sont pas couverts.

Les dépôts contenant ces cas restent directement ouvrables, mais chaque élément incompatible ne peut pas être annoncé comme fonctionnel. La matrice détaillée est dans [CLAUDEX.md](CLAUDEX.md).
