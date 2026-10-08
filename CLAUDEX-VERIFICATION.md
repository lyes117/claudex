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

Apres reconstruction des preconditions CLI et du probe Windows natif, la reprise
CLI/Windows donne 12/15 reussites : les huit tests CLI passent, deux tests Windows
echouent sur la copie d'un auxiliaire partage (os error 32) et un expire. Avec un
seul worker, les deux cas de refus passent (9,603 s et 45,258 s), tandis que le
cas metadata expire encore a 60 s. Aucun delai ou reglage de securite modifie.
Les auxiliaires stages et compiles ont des hashes identiques ; la cause du
timeout reste ouverte (`tests-core-cli-windows-prerequisites-cycle1.log`,
`tests-windows-elevated-isolated-cycle1.log`). Ces reprises ne remplacent pas une
suite complete verte.

La fixture de refus hors roots ne depend plus du message anglais du systeme :
elle observe l'echec natif, exclut les codes negatifs et le timeout 124, et exige
NotFound pour les deux fichiers. Les cinq cas workspace_roots passent, y compris
les positifs et les metadata protegees (`tests-workspace-denial-locale-cycle2.log`,
9,347 s). Une autre erreur positive du shell reste possible ; les controles
positifs utilisent des fixtures independantes. Formatage et Clippy core reussis.
Il s'agit de controles natifs avec modele de fixture, sans modification moteur.

La compression du seul ancien cache incremental, avec liens refuses et chemin
verifie dans le fork, termine a zero : 214912 fichiers, 107223041064 octets
logiques stockes dans 52248551183 octets (`compact-incremental-cache-cycle1.log`).
Elle libere environ 51,2 Gio sans supprimer ces caches. Les builds suivants
conservent CARGO_INCREMENTAL=0 ; les sources et l'installation restent distinctes.

## Capture du plafond et livraison de l'interface — 3 octobre 2026

Le DTO de plafond et les lecteurs d'en-tête stricts sont commités (0e67c23,
991bd2c). La tranche suivante capture le plafond effectif à la création,
préserve un `null` explicite pour le rejeter, valide avant mutation et transmet
le même snapshot lors des reverts. La suite protocol/rollout/thread-store
passe 760/760 (`tests-tool-policy-carrier-cycle3.log`). La reprise consolidée
passe ensuite 777/777 en 65,367 s (`tests-claudex-pre-install-cycle1.log`) :
les derniers ajouts du deuxième revert et de capture native core, huit cas de
schéma et huit cas natifs de bridge sont inclus.
Cela ne restaure pas encore l'autorité lors d'une reprise froide ; aucun profil
Claude contraint n'est activé par cette seule capture.

La nouvelle interface (bannière, compositeur, marqueurs de transcript et outils)
est dans les sources. La revue statique ne trouve pas de P0/P1 sur ce delta.
Son premier cycle effectivement exécuté donne 358/454 réussites et 96 échecs
(`tests-claudex-shell-cycle4.log`) : 78 cas de rendu à vérifier et 18 assertions
révélant une régression réelle de Sparkle avec le fond Reset. Le calcul du fondu
est corrigé et revu. Après inspection des captures et déroulement des tests à
plusieurs états, le cycle 22 normal passe 454/454 en 10,308 s
(`tests-claudex-shell-cycle22.log`). Aucun snapshot n'est en attente ; aucune
assertion n'est forcée. La suite TUI complète reste à reprendre.

Formatage et Clippy ciblé réussissent (`fmt-claudex-ui-delivery-cycle1.log`,
`fix-claudex-ui-delivery-cycle1.log`). Les seuls changements de formatage hors
périmètre et l'import de fixture Windows supprimé par Clippy sont restaurés
après inspection. Commits : 2481557 pour la capture, 33a1cb9/5e19824/2caf7d7
pour l'interface et 76b247c pour son golden inline déjà vérifié.

Build natif réussi en 4 min 27 s, installation réussie. Les quatre exécutables
installés correspondent aux sources ; nouveau hash principal :
**686F915909ED8517B19BB052ADE931C751BDCC290D2D44EEB08DF00E83D96A36**.
Receipt : `installed-claudex-ui-delivery-hashes-cycle1.json`. Le build commence
à 2caf7d7 avec des modifications privées de workflow non commités ; ces modules
restent sans appel de production. Les fichiers config/auth contrôlés ont les
mêmes empreintes immédiatement avant/après installation, sans copie de secrets.
PowerShell sans profil et CMD avec PATH frais lancent le binaire et confirment
ChatGPT (`launch-claudex-ui-powershell-cycle2.log`, `launch-claudex-ui-cmd-cycle2.log`).
Le cycle PowerShell précédent présente un défaut CLIXML du harness, corrigé par
sortie texte explicite ; il n'est pas compté comme preuve propre.

Le terminal Windows réel affiche la nouvelle bannière et le prompt, ouvre
/help, colle « étude 研究 » sur deux lignes et ferme normalement à code 0.
Preuve : `live-ui/run-d677344f7d9b40168364135f9e113d6f/verified.json`.
La réponse assistant avec le marqueur ● est observée ; l'inférence CLI séparée
valide exactement NO_TOOLS_OK via ChatGPT, sans override du catalogue officiel
(`no-tools-live-ui-delivery-cycle1.log`, `live-no-tools/run-TQd1SW/verified.json`).
Read/Grep/Glob sont ensuite exécutés réellement en direct et CodeMode, avec
cartes natives restaurées (`file-tools-live-ui-delivery-cycle1.log`,
`live-file-tools/run-1dGo4e/verified.json`, catalogue de fixture explicite).
Cela ne démontre pas tous les raccourcis, le rendu de chaque écran ni l'arrêt de
tous les processus externes. Des menus et textes promotionnels Codex subsistent.

Le nouveau bridge de workflows natifs reste privé et sans outil enregistré.
Ses 13 premiers tests exécutés donnent 9 réussites et 4 échecs ; après correction,
ses 16 cas passent dans la reprise consolidée.
La perte de propriété sur erreur de flush et l'écart de transport strict sont
corrigés ; leur reprise est incluse dans les 777 contrôles. La revue trouve
encore une course de capture d'Arc après admission et un arrondi numérique f64,
documentés dans le design. Ils bloquent l'activation. La notification brute au
parent avant validation, le runner borné et la reprise après panne restent
des étapes séparées. Ces résultats ne constituent pas une parité complète.

## Reprise ciblée et mémoire native — 3 octobre 2026

La campagne core ciblée `tests-claudex-native-ownership-numeric-cycle3.log`
passe **110/110**, en 57,789 s. Elle comprend les dix cas de propriété exacte
du runtime enfant et les treize cas de schéma/decimaux. L'admission conserve
l'Arc original, y compris pour l'envoi initial et le cleanup ; les nombres sont
comparés exactement avec des budgets de lexème/exposant. Le cycle1 était une
erreur de compilation de fixture ; le cycle2 passait 109/110 avant correction
d'une fixture utilisant des registres distincts. Les autres barrières
d'activation du workflow restent ouvertes : notification parent avant
validation, runner JS borné et journal/reprise native.

La suite TUI complète réellement exécutée (`tests-claudex-tui-full-cycle1.log`)
donne **5248 réussites et 314 échecs**, sur 5562 tests, avec huit skips.
270 réussites portent l'indicateur LEAK de nextest ; le nettoyage des processus
de ces fixtures reste à établir. 267 échecs concernent des snapshots et 47
des assertions. La revue distingue les marqueurs intentionnels de deux
régressions réelles : ancien marqueur du prompt épinglé et bannière trop haute
sur petit terminal. La reprise ciblée
`tests-claudex-tui-failure-recovery-cycle2.log` exécute 323 cas :
276 réussites (23 LEAK), 47 échecs, 5249 skips ; compilation 5 min 58 s,
tests 71,602 s. Les nouveaux cas compact à 48×16/48×12, les métadonnées,
les cellules du compositeur et l'égalité du prompt épinglé passent.

Les 47 échecs sont 43 assertions Insta, trois fixtures cherchant encore les
anciens glyphes et une vraie régression de couleur du prompt vocal animé.
Les conditions de l'animation reconnaissent maintenant ❯/● tout en gardant
Red/BOLD et le comportement des anciens glyphes ; les assertions couleur
ne sont pas modifiées. Cette correction reste à exécuter.

Après revue, 37 goldens externes et six inline sont acceptés sélectivement.
Trois attentes inline sont d'abord rebasées par cargo-insta sans modification
du littéral malgré un code de sortie zéro. Leurs payloads sont revérifiés,
puis leurs clés réelles acceptées ; changements de hash source et inventaire
vide sont confirmés. Le contenu des 37 fichiers externes reconstruit exactement
les bytes approuvés en rétablissant seulement `assertion_line`, métadonnée
normalement retirée par insta. Receipts :
`claudex-tui-review-goldens-cycle2.json`,
`claudex-tui-rebased-inline-accept-receipt-cycle2.json` et
`claudex-tui-final-acceptance-proof-cycle2.json`.
Les assertions suivantes et la suite TUI complète doivent encore passer.

L'intégration claude-mem est en construction, **pas installée ni validée en
inférence réelle**. Le bundle est construit depuis la révision publique épinglée
`a1951f2ad247330b2b5d58a1e0c7efeef4a03be5`, édition
`13.29.0-dev+a1951f2`. La release npm 13.28.0 examinée ne contient pas le
fournisseur Codex requis et est refusée par le helper. Les quatorze tests Node des
helpers passent ; les fixtures source du fournisseur donnent 53 réussites,
16 skips Windows et zéro échec. Les scripts lifecycle des dépendances n'ont
pas été exécutés pendant le build.

Le design utilise l'app-server officiel et un observer séparé sans outils ni
hooks, avec HOME/USERPROFILE privés pour son processus. Les nouvelles données
sont prévues sous `~/.claudex/memory/data`, port 37778, sans import ou modification
de l'état Claude-mem existant. Le chargement effectif du plugin natif doit
être prouvé avant exclusion du plugin legacy ; le marqueur seul ne suffit pas.
La capture, l'injection, la recherche MCP, l'isolation réelle et l'authentification
observer sur deux projets synthétiques restent à vérifier. L'identité par nom
de dossier, le matcher PreToolUse partiel, la redaction non exhaustive et
l'allocation de port non atomique restent des limites à traiter.

La sélection mémoire passe ses onze tests ciblés de configuration dans
`tests-claudex-memory-selection-config-cycle2.log`. La campagne élargie
`tests-claudex-memory-reporting-libraries-cycle1.log` est arrêtée à la compilation :
17 diagnostics dans les nouvelles fixtures de reporting Workflow, aucun test
exécuté. Les imports de macro, deux conversions d'erreur `AgentPath` et deux
snapshots d'environnement sont corrigés ; les refus et résultats invalides
vérifient désormais l'erreur exacte et l'exécution de la requête native.
La politique privée est capturée avant startup et le fallback V2 conserve son
refus de changement de propriétaire. Ces corrections attendent leur exécution ;
le bridge Workflow reste privé, non activé et non installé.

Le lancement installé est revérifié le 3 octobre : `claudex 0.160.0` en
PowerShell et CMD, connexion `Logged in using ChatGPT`, binaire présent et entrée
PATH utilisateur présente. Le processus d'outillage hérite encore de l'ancien
PATH ; le contrôle recharge les PATH Machine et User sans les modifier.
Cette vérification concerne le binaire installé précédent, pas les sources
actuellement en cours de validation.

## Résultats les plus récents — 3 octobre 2026

Cette section actualise les points encore marqués « à exécuter » dans les
comptes rendus historiques ci-dessus. Une nouvelle installation est décrite
après les campagnes source ci-dessous.

- `tests-claudex-memory-reporting-libraries-cycle2.log` : **3274/3278 PASS**,
  trois skips. Deux fixtures de reporting ont ensuite été corrigées pour
  attendre la fin du traitement avant de vérifier la version V1 ; une fixture
  skills a reçu un HOME synthétique plutôt que le profil réel. Le dernier
  échec exige un privilège Windows de création de symlink (OS 1314).
- `tests-claudex-recovery-cli-memory-tui-cycle1.log` : **368/372 PASS**,
  22 indicateurs LEAK et 8637 cas hors filtre. Les 17 cas de reporting privés,
  cinq tests du CLI mémoire, le watcher et la fixture skills corrigée passent.
  Les quatre échecs sont des snapshots TUI ; leur revue indépendante trouve
  exactement huit substitutions de glyphes. Les assertions suivantes et la
  suite TUI complète restent à rejouer.
- `tests-claudex-recovery-cli-memory-tui-cycle2.log` : **370/372 PASS**,
  24 indicateurs LEAK. Les quatre snapshots revus sont acceptés avec preuve
  exacte et hashes avant/après. Deux tests atteignent ensuite une nouvelle
  assertion snapshot (espacement du feedback copie et étape exec suivante) ;
  leurs nouveaux artefacts attendent leur revue, sans acceptation automatique.
- `tests-claudex-memory-identity-query-cycle2.log` : **31/31 PASS**, sans skip,
  incluant identité canonique Git/worktree/submodule/jonctions, contenu HTTP
  synthétique scoped, budgets de lecture et télémétrie explicitement désactivée.
- `build-claudex-memory-cx1-cycle1.log` : bundle source construit avec succès,
  édition `13.29.0-dev+a1951f2.cx1`. Resolver et artefacts sont hashés ; l'ancien
  bundle sans schéma cx1 est désormais refusé. Aucun service, capture,
  compression, injection ou inférence réelle de ce bundle n'a été vérifié.

La notification brute des enfants supervisés est maintenant supprimée par une
politique privée immuable avant publication ; les fixtures de résultats
valides/invalides, warm reuse et FullHistory fork passent. Le fallback V2 est
revu statiquement. Le restore froid de cette politique, le runner JS borné,
le journal/reprise et l'outil conversationnel Workflow restent ouverts.

`tests-claudex-memory-cx2-combined-cycle1.log` passe **34/34**, sans skip.
Le build `build-claudex-memory-cx2-cycle1.log` et les hashes/resolver du bundle
cx2 sont vérifiés. Cette édition refuse le reclaim automatique de port ;
elle n'est pas installée ni exécutée en inférence réelle.

`just fmt` termine avec succès. `just fix` scoped aux six packages termine
avec succès après 11 min 08 s ; il conserve un warning dans le cleanup privé
de spawn (verrou de publication gardé pendant SQL Closed). La revue confirme
que libérer ce verrou réintroduirait une course avec le remplacement ; la
contrainte de non-réentrance du store doit être documentée. La correction
automatique retire un import inutilisé dans core/tests/suite/scenarios.rs.

Le build natif et ses helpers passe en 4 min 33 s. Le binaire est installé,
SHA identique à la source `5E0D4685…0F03217C` ; les fichiers d'installation
précédents remplacés sont conservés dans un backup owned. PowerShell et CMD
confirment la version et `memory status` non configuré ; la connexion ChatGPT,
`-C`, aide native mémoire et refus d'option modèle sont vérifiés.
Receipt : `installed-claudex-memory-reporting-cycle1.json`.

Le contrôle Windows PTY réel du binaire installé vérifie bannière Claudex,
ouverture de /help, paste Unicode multiligne et sortie zéro, sans envoyer
de turn modèle. Receipt sous `live-ui-memory-cycle1/run-aa66094c54ac417a90075b4dd1fc7c42`.
59 warnings de configuration sont affichés au démarrage du profil courant ;
leurs causes ne sont pas classifiées par ce smoke. Des entrées et raccourcis
Codex persistent, et ce résultat ne prouve pas la parité Claude Code complète.

La reprise finale `tests-claudex-tui-exec-final-recovery-cycle5.log` passe
le test d'historique jusqu'à son étape 6. Les attentes step5/step6 sont acceptées
avec revue indépendante des deux glyphes et preuve exact-hash par paire.
Le contrôle follow/caret/geometry passe dans le cycle3 (un indicateur LEAK).

La suite TUI complète courante, `tests-claudex-tui-full-cycle2.log`, termine
avec **5554/5554 PASS, zéro échec et quatre skips**, en 665,085 secondes.
Un cas est marqué slow et 304 sont marqués LEAK par nextest. Le nettoyage de ces
fixtures reste à examiner ; zéro assertion en échec ne prouve pas zéro ressource
survivante. Cette campagne concerne la lib codex-tui dans le profil dev-small,
pas toutes les intégrations ou tous les packages du workspace.

Le proxy/guard/harness mémoire possède désormais 28 tests synthétiques PASS,
deux lanceurs natifs compilés et une revue indépendante. Les raccords sidecar,
checkpoints et bornes sont vérifiés ; le verdict live reste explicitement faux
jusqu'à supervision et association effective capture/invocation/audit. Aucun
auth réel n'a été lié et aucune inférence mémoire réelle exécutée.

## Candidat workflow et diagnostic hooks — 3 octobre, cycle routing-workflow-1

Le build natif `dev-small` termine sans erreur en 5 min 16 s. Le candidat est livré dans `.build-tools/candidate-routing-workflow-cycle1/bin`, sans modification du PATH utilisateur. La session installée était ouverte (PID 34452) ; Windows a refusé le remplacement de son exécutable. Aucune session utilisateur n'a été interrompue. Le candidat se lance dans PowerShell et CMD et reconnaît l'authentification ChatGPT existante.

`node scripts/workflows.test.mjs` passe après le nouveau profil. Le vrai CLI compilé a ensuite exécuté `text-only.workflow.js` : deux phases, trois résultats structurés issus d'appels réels, fichier témoin inchangé. La deuxième invocation du même CLI et run-id reprend trois résultats en cache, avec résultat et checkpoint identiques. L'arrêt et l'absence du verrou actif sont vérifiés. Receipt : `.build-tools/workflow-zai-cycle1/cli-text-only-a1da7862-c43b-4051-a180-e76122c68706.receipt.json`. Concurrence observée : un. Aucun outil `Workflow` conversationnel, agent natif imbriqué, accès fichier par le modèle ou appel Z.ai n'est démontré par ce test.

Les bibliothèques de candidature au routage, de construction Chat Completions et de décodage GLM passent 238 tests ciblés, zéro skip, puis Clippy avec warnings interdits. Elles restent inactives : aucun fournisseur GLM natif, transport réseau, authentification Z.ai ou imputation au quota Coding Plan n'est encore validé.

Le diagnostic sans tour modèle depuis `C:\Users\lyesb` observe 48 notifications de compatibilité, toutes classées `unsupported-agent-tools`. Le catalogue natif contient sept hooks activés et de confiance, dont une commande `node` sans arguments et une commande `bash`. Aucun hook n'est exécuté par ce diagnostic. La lecture du code confirme que le champ `args` d'un hook SEO est ignoré ; correction en cours. La résolution Windows de Bash, les doublons GitNexus et les différences de payload nécessitent encore leur vérification propre. Ces causes possibles ne sont pas présentées comme des échecs reproduits.

## Limites matérielles de compatibilité

1. La nouvelle bannière, le compositeur et les marqueurs Claudex sont installés et vérifiés. Des menus, textes et raccourcis de Codex restent présents ; l'ensemble des interactions Claude Code n'est pas encore reproduit.
2. Les métadonnées de sécurité Claude non applicables au moteur natif entraînent le rejet de l'agent ou du skill concerné, plutôt qu'une exécution avec des permissions fictives. Les règles `ask` bloquent ; certains refus de chemins bloquent conservativement l'outil entier.
3. Les hooks gardent les payloads natifs Codex. Les hooks HTTP, prompt et agent, ainsi que les événements absents du moteur natif, ne sont pas pris en charge. Un script exigeant un payload Claude exact doit être adapté.
4. Les plugins locaux activés sont lus depuis leur registre et leurs répertoires standards. Les chemins personnalisés de manifeste, téléchargements et marketplace Claude ne sont pas implémentés.
5. Les workflows sont séquentiels, avec checkpoints, pause/reprise/arrêt depuis la CLI ou le panneau et appels Codex réels. L'outil conversationnel natif `Workflow`, les workflows imbriqués, la relance individuelle, la concurrence et la validation complète des schémas Claude restent ouverts. Le runtime JS exécute des programmes de confiance ; `node:vm` ne fournit pas une isolation de sécurité.
6. Le tableau `/agent-center` et `claudex agents` fonctionnent maintenant sur le serveur embarqué installé. Quatre fixtures natives couvrent la découverte, deux sessions vides avec brouillons, archive/delete et la lecture d'un writer externe (deux serveurs dans le même processus). Le terminal réel confirme tableau, deux créations, ouverture/retour et fermeture normale. Le contrôle live entre processus et les tâches détachables restent non prouvés. Voir CLAUDEX-AGENTS-NAVIGATION-DESIGN.md pour les suites non vertes et les journaux.
7. SSE MCP, imports Markdown globaux `~`, activation dynamique exacte des règles par chemins et `${CLAUDE_PROJECT_DIR}` dans les corps de commandes ne sont pas couverts.

Les dépôts contenant ces cas restent directement ouvrables, mais chaque élément incompatible ne peut pas être annoncé comme fonctionnel. La matrice détaillée est dans [CLAUDEX.md](CLAUDEX.md).

## Rebuild accueil, commandes et hooks — reprise du 4 octobre

L'installation précédente reste conservée, avec une sauvegarde vérifiée sous
`.build-tools/backups/installed-before-hooks-slash-cycle2`. La fermeture de la
session utilisateur autorise son remplacement ; le rebuild attend les contrôles.

Les bibliothèques hooks/catalogue passent 1000 tests ; la bibliothèque app-server
passe 369 tests et les trois intégrations RPC ciblées passent. Le cycle TUI 5
échoue : 5517 tests réussis, 45 échecs et un délai dépassé. Après corrections,
le groupe de reprise du cycle 6 passe 77 tests sur 80 : le test d'horloge et deux
débordements de pile Windows restent à corriger. Deux tests passent après retry ;
37 sont marqués LEAK. Ce résultat ne valide pas la suite TUI entière.

La revue indépendante du cycle 6 compare 1351 goldens préservés à 1352 goldens
courants : 16 changements d'accueil déjà relus, un nouveau rendu framebuffer,
aucune suppression ni acceptation sans rapport. Reçu :
`.build-tools/claude-tui-visual-cycle2/golden-review-cycle6.json`.

La commande `/tui classic|fullscreen|scrollback` et le réglage conditionnel
`never` vers `auto` sont intégrés aux mécanismes natifs, après revue adverse.
Les politiques enregistrées `always` et `auto` sont préservées malgré les
restrictions CLI/SSH ; le renderer actif ne bascule pas avant redémarrage.
Les tests de cette tranche et son rendu picker ne sont pas encore validés.

Le moteur V8 de workflows, le superviseur Windows et les raccourcis compatibles
restent des tranches préparées séparément. Ils ne sont pas activés par ce rebuild.
Ni les workflows conversationnels naturels ni la parité complète du TUI ne sont
annoncés comme terminés.

### Reprise cycle 7 : preuves et tranches préparées

Le groupe ciblé passe 93 tests sur 94 (deux retries, 37 marqueurs LEAK). Le fork,
le délai de commande, les six fixtures `/tui` et les cinq fixtures Ctrl+O passent.
Le dernier échec est une reprise froide qui déborde la pile pendant la restauration
du moteur ; le correctif minimal de frontière de future est en test. La suite TUI
complète, les quatre tests du réglage conditionnel, Clippy et le rebuild restent
à exécuter avant le remplacement de l'installation.

La revue indépendante des goldens cycle 7 approuve 17 rendus existants modifiés
et un nouveau sur 1352, sans suppression. Le seul nouveau changement depuis le
cycle 6 est le picker `/tui`, inspecté à 40 et 80 colonnes. Reçu :
`.build-tools/claude-tui-visual-cycle2/golden-review-cycle7.json`. Cela ne remplace
pas une vérification dans un terminal Windows réel.

La checklist Ctrl+T native, avec rejet des événements d'un autre thread avant
toute mutation, est préparée et relue dans le staging cycle 5 ; elle n'est pas
encore appliquée. Le parseur AST de workflows réutilise Tree-sitter et sa grammaire
JavaScript officielle, afin de distinguer les imports réels du texte des prompts.
Cette tranche reste stagée, sans compilation ni exécution de `film.workflow.js`.

L'ancien cache incrémental généré a été conservé sur D: ; le nombre de fichiers
(247159) et le total d'octets (109730196364) correspondent exactement à l'inventaire
avant déplacement. Aucun code, dépôt utilisateur ou fichier d'authentification
n'a été déplacé. Le builder désactive ce cache par défaut et respecte une valeur
explicitement fournie. Reçu : `.build-tools/cache-archive-recovery-cycle7.json`.

### Checklist Ctrl+T appliquée : validation ciblée cycle 7g

Les 33 fichiers de la tranche checklist ont été appliqués après contrôle de tous
les hashes avant/après, avec sauvegarde dédiée et reçu
`.build-tools/claude-tui-plan-panel-cycle5/application-receipt.json`.
La compilation et les neuf nouvelles fixtures passent, ainsi que les cinq fixtures
Ctrl+O/raccourcis : 14 tests réussis sur 15 dans
`.build-tools/tui-resume-core-diagnostic-checklist-cycle7g.log`.
Le seul échec reste la reprise froide ; les diagnostics la localisent désormais
dans la restauration de l'historique de session, après son initialisation.

Les goldens de checklist à 32 et 80 colonnes sont issus des fixtures réellement
exécutées. Leur revue indépendante, le schéma de configuration et la suite TUI
complète restent nécessaires. Cette checklist affiche le plan natif reçu ; elle
n'implémente pas encore les tâches partagées persistantes de Claude. Aucun nouveau
binaire installé ni test du nouveau TUI dans un terminal Windows réel à ce stade.

Le parseur AST privé de workflows passe les revues statiques adverse et Rust,
mais reste stagé. Une tranche distincte remplace deux sérialisations non bornées
du bridge par un writer compteur ; ses huit fixtures sont préparées, sans test
exécuté ni changement actif. L'outil Workflow conversationnel reste à raccorder.

### Reprise après redémarrage : preuves du 4 octobre

Le journal principal et ses 26 journaux d'agents ont été intégralement analysés
comme JSONL. Les changements préexistants et l'installation ont été préservés.
L'inventaire et le checkpoint sont dans `.build-tools/recovery-2026-10-04*`.

La tranche de sérialisation bornée du bridge de workflows est maintenant appliquée,
après vérification des hashes et deux revues indépendantes. Ses huit nouvelles
fixtures et le cas existant de budget collectif passent : **9/9**, exit 0,
dans `.build-tools/recovery-workflow-bounds-cycle4-tests.log`. L'hôte V8,
le superviseur Windows et l'outil Workflow conversationnel restent à intégrer.

Le diagnostic natif de la reprise froide identifie un cadre RPC de 4,44 Mio.
Le boxing de la future interne de commande Claude réduit ce cadre de 128 224
octets. La reprise et les deux fixtures de délai passent au cycle7n : **3/3**,
avec un signalement `LEAK` pour la reprise. La preuve après nettoyage, le schéma,
la suite TUI et le nouveau build restent à valider ; ce résultat ne modifie
pas le binaire installé.

L'audit final demandé couvre Claude Code, les capacités Codex préservées et
le routage Z.ai. Son protocole est dans `CLAUDEX-FINAL-FACTCHECK-PLAN.md`.
Il reste à exécuter après la fusion ; aucune parité complète n'est déclarée.

### Avancement vérifié du 4 octobre — intégration encore ouverte

La compilation de production CLI/app-server/TUI passe après le correctif RPC et
l'import explicite de reprise (cycle7q). Le schéma a été réellement régénéré,
avec revue de son diff. Les trois fixtures RPC d'expansion, hooks argv et
configuration gérée passent (3/3).

La suite TUI complète cycle7q a exécuté 5593 tests : 5545 passés, 46 échecs,
2 timeouts, 8 ignorés ; 300 résultats ont le signalement LEAK. Les fixtures
obsolètes et les captures Ctrl+O/F6 ont ensuite été relues et corrigées.
Le passage ciblé isolé cycle7s donne 44/49 passés (dont 8 LEAK), cinq captures
encore en échec ; les deux anciens timeouts passent en exécution séquentielle.
Une deuxième vague de cinq captures a été inspectée et appliquée ; son rerun est
en cours. Cela ne clôture pas la suite complète ni l'origine des LEAK.

Les fondations Workflow codec/AST et les lots Windows 03/04/04b/05 sont appliqués.
Cargo metadata --locked et le véritable bazel-lock-update passent. Le check
production PTY et dix fixtures Job Windows passent : 10/10, exit0.
Les tests AST ont trouvé une particularité de grammaire sur export suivi d'un
commentaire/newline ; les corrections restent soumises aux tests avant le lot V8.
L'hôte V8 supervisé, son caller conversationnel, le journal et la reprise restent
ouverts. Aucun film ni workflow utilisateur réel exécuté.

Le transport Chat Completions Z.ai dédié est intégré et les 30 fixtures ciblées
DTO/decoder/HTTP/framing/stream passent. La suite complète API/HTTP précédemment
exécutée conserve cinq échecs de fallback TLS Windows Schannel : les deux
expériences de fixture n'ont rien corrigé et ont été intégralement retirées.
Aucune politique de certificat/TLS n'a été assouplie. L'interface transport est
exportée, mais provider/identité enfant/replay/spawn/quota réel restent ouverts.
Aucun appel d'inférence distant et aucun remplacement du binaire installé.

### Hôte Workflow privé et bibliothèques — preuves complémentaires

Les 174 tests des bibliothèques PTY, code-mode-runtime et model-provider-info
passent (1 ignoré), dont les neuf fixtures d'admission Z.ai préparatoire et les
deux fixtures de reçu de lancement Windows. Les 18 essais de l'hôte V8 et du
superviseur Windows passent (1 ignoré). Ils couvrent parsing sans évaluation,
imports, groupes synthétiques, finalisation, refus des nombres non représentables,
EOF, délai CPU/microtasks et allocation ArrayBuffer sous limites combinées.
Les agents natifs, le journal, la quarantaine et la reprise ne sont pas prouvés
par ces essais. Aucun workflow film réel exécuté et aucune inférence distante.

La correction finale de footer running a conservé métadonnées/EOL ; une nouvelle
suite complète du package TUI est en cours (cycle7u, 4 threads, aucun retry,
INSTA_UPDATE=new, profil utilisateur uniquement isolé dans le process runner).
Aucun résultat forcé et aucune limite de temps accrue. Binaire installé inchangé.
