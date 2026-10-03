# Claudex

Fork local de `openai/codex`, base `rust-v0.160.0` (commit `a956835d020762cb2b570053af06f643a11c0ecc`). Branche `claudex/main`, distant `upstream`. Le fork n'est pas publié sur GitHub. Licence Apache-2.0 du moteur Codex conservée. Aucun code propriétaire de Claude Code repris.

## Utilisation

```powershell
claudex
claudex login status
claudex exec "Explique ce dépôt"
claudex compat .
claudex workflow chemin/film.workflow.js --args '@arguments.json' --run-id mon-film
claudex workflow list
claudex workflow pause mon-film
claudex workflow resume mon-film
claudex workflow stop mon-film
```

L'exécutable natif est installé dans `%LOCALAPPDATA%\Programs\Claudex\bin`, ajouté au PATH utilisateur. Ouvrir un nouveau terminal après installation. L'état d'authentification et la configuration Codex restent dans le dossier Codex officiel. L'authentification ChatGPT est le mode par défaut ; aucune clé API Anthropic n'est utilisée. Les limites et quotas du compte Codex s'appliquent.

## Compatibilité implémentée

| Source existante | Comportement | Limites |
|---|---|---|
| `CLAUDE.md`, `CLAUDE.local.md`, `.claude/CLAUDE.md` | Instructions chargées avec les instructions Codex, sans copie | Imports relatifs Markdown directs ; imports globaux `~` et activation dynamique complète non couverts |
| `.claude/rules/**/*.md` | Règles ajoutées aux instructions, budget borné | `paths` est une condition donnée au modèle, pas un filtre d'exécution garanti ; profondeur 6, 1000 fichiers |
| `.claude/agents/*.md` | Nom, description et instructions alimentent les rôles natifs | Modèles Claude ignorés ; `tools`, `disallowedTools`, `permissionMode`, `hooks`, `isolation` entraînent un refus explicite du rôle |
| `.claude/skills`, `.claude/commands` | Découverte native ; `/nom arguments` développe le Markdown dans le terminal | Builtins Codex prioritaires ; pas de menu Claude exact ; injection shell `!` refusée ; `context`, `agent`, `hooks`, `disallowed-tools` refusés |
| Arguments de commandes | `$ARGUMENTS`, `$ARGUMENTS[N]`, `$N`, arguments nommés, guillemets shell | Substitution locale en mémoire ; pas de syntaxe privée Claude |
| `settings.json`, `settings.local.json` | Environnement, permissions et plugins lus sur place | Paramètres de modèle/API Claude non repris ; configuration native et politique gérée conservées |
| Permissions `deny`, `ask` | Refus avant dispatch, puis après modification par un hook | `ask` devient refus ; règles de fichiers bloquent conservativement l'outil entier ; syntaxe spécialisée et isolation Claude non équivalentes |
| Hooks | Événements supportés vers le moteur de hooks natif ; contexte projet/plugin fourni | Revue de confiance native conservée ; hooks prompt, agent, HTTP et événements non supportés ne sont pas exécutés ; shell Windows natif, pas de traduction automatique Bash |
| `.mcp.json`, MCP dans `.claude.json` | Configuration stdio et HTTP vers le gestionnaire MCP natif | SSE non supporté ; OAuth natif Codex, tokens Claude non importés ; autorisations interactives Claude non identiques |
| Plugins installés explicitement activés | Répertoires skills, commands, agents, hooks et MCP standards | Aucun téléchargement ; chemins non standards dans le manifeste non résolus ; pas de marché Claude complet |
| Workflows JS | `agent`, `parallel`, `pipeline`, `phase`, `log`, `args`, sortie structurée, journal, pause/reprise/arrêt et reprise après modification | Agents séquentiels, processus Codex distincts ; pas d'outil natif `Workflow`, de workflows imbriqués, ni de relance individuelle depuis le panneau |
| Outils `Read`, `Glob`, `Grep` | Implémentations natives originales via le système de fichiers de la session ; refus, hooks et CodeMode conservés ; cartes TUI restaurables ; installation et inférences ChatGPT vérifiées ; annulation et affichage après PostToolUse vérifiés par intégration | Modes direct et CodeMode sélectionnés par un catalogue de fixture local pour le test ; UTF-8 jusqu'à 1 Mio, sorties de 8 Kio, recherches bornées ; PDF/images, Grep multiligne et événements CLI JSON non couverts ; aucune garantie de persistance après arrêt brutal |

Le TUI possède un en-tête Claudex adapté aux terminaux larges et étroits, un compositeur délimité et cinq entrées : `/help` (aide recherchable), `/agents` (catalogue des rôles chargés, en lecture seule), `/tasks` (sous-agents de cette session), `/workflows` (exécutions locales, détails et contrôles) et `/agent-center` (tableau Codex partagé). Les outils, sous-agents, sessions et extensions natifs restent présents. Le terminal fonctionne en mode embarqué : `/agent-center` nécessite un serveur partagé explicitement connecté ; le daemon et ses files ne sont pas activés localement pour éviter de rejoindre ou remplacer votre serveur Codex officiel. Ce n'est pas encore une reproduction intégrale des écrans, raccourcis et gestionnaires Claude. Les restrictions Claude dont l'équivalence n'est pas établie ne constituent pas une garantie générale d'isolation.

Les payloads de hooks gardent leurs noms et structures natifs Codex, avec les aliases de matching déjà fournis par Codex ; un hook qui exige exactement un payload Claude `Write`/`Edit` nécessite encore une adaptation. Les instructions Claude ont un budget de 32 Kio et peuvent être tronquées. `${CLAUDE_PROJECT_DIR}` dans un corps de commande est refusé jusqu'à l'ajout d'un contexte de session explicite.

Les fichiers de configuration sources ne sont ni migrés ni dupliqués. Les messages de diagnostic de compatibilité n'affichent pas les valeurs d'environnement, d'en-têtes ou de secrets. Les journaux ordinaires de Codex et les résultats de workflow restent des données locales potentiellement sensibles.

## Workflows retrouvés

Le véritable `film.workflow.js` est dans `C:\Users\lyesb\Desktop\Studyshare\marketing\marketing studyshare\moteur\creation`. Il emploie le contrat JavaScript documenté (`agent`, `parallel`, phases et paramètres), initialement exécuté par le runtime propriétaire Claude. Le lanceur Codex déjà présent dans ce dépôt a été étudié ; ses modifications préexistantes sont préservées. Le nouveau runtime est une implémentation originale du contrat public, intégrée à la commande compilée du fork.

Les scripts workflow sont des programmes de confiance : `node:vm` n'est pas un bac à sable de sécurité. Les agents sont appelés par l'exécutable Claudex et utilisent l'authentification officielle. Une reprise exige les mêmes arguments et le même répertoire. Après modification du script, le préfixe d'appels identiques est rejoué ; le premier appel différent ou échoué invalide les résultats suivants. Un agent échoué retourne `null` au script et n'est pas mis en cache. L'horloge implicite et `Math.random` sont refusés pour préserver ce contrat ; une date explicitement fournie reste possible.

La pause laisse finir l'agent actif puis bloque les suivants ; la reprise libère cette file. L'arrêt annule le processus enfant actif et conserve les checkpoints déjà enregistrés. Les verrous protègent contre une deuxième exécution du même identifiant. Le panneau `/workflows` se rafraîchit et affiche les phases, agents et états conservés dans `~/.claudex/workflow-runs/<run-id>`. Il ne lance pas encore de nouveau workflow. Une panne après un effet externe mais avant le checkpoint peut nécessiter une revue humaine ; la reprise ne garantit pas une transaction distribuée.

## Reconstruction

Rust 1.95.0, MSVC et Windows SDK, Node 22. Les outils de développement téléchargés sont dans `.build-tools` et ne sont pas versionnés. Les exécutables auxiliaires inchangés sont réutilisés depuis le paquet officiel local **de la même version 0.160.0** (code-mode, sandbox Windows, ressources voix et ripgrep) ; l'exécutable principal est compilé depuis ce fork.

```powershell
$env:Path = "$PWD\.build-tools\bin;$PWD\.build-tools\pwsh;$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location codex-rs
cargo build -p codex-cli --bin codex --profile dev-small
Pop-Location
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/install-claudex.ps1
node scripts/workflows.test.mjs
just test -p codex-config -p codex-agent-roles -p codex-home -p codex-skills-extension -p codex-hooks -p codex-tui -p codex-cli --cargo-profile dev-small
```

Le nom du binaire de compilation reste `codex` pour conserver les outils internes amont ; le fichier installé est le véritable binaire natif `claudex.exe`. Les mises à jour automatiques amont sont désactivées pour éviter de remplacer le fork par Codex standard.

## Sources

- [Code et installation Codex](https://github.com/openai/codex)
- [Authentification officielle Codex](https://learn.chatgpt.com/docs/auth)
- [Configuration Claude documentée](https://code.claude.com/docs/en/settings)
- [Hooks](https://code.claude.com/docs/en/hooks), [agents](https://code.claude.com/docs/en/sub-agents), [skills](https://code.claude.com/docs/en/skills), [workflows](https://code.claude.com/docs/en/workflows)

Les vérifications réellement exécutées sont consignées dans `CLAUDEX-VERIFICATION.md`.
