# Claudex : cycles de compatibilité

Objectif : un fork installé, avec une interface terminal familière et des comportements réellement exécutés par Codex. Référence publique : documentation Claude Code ; aucun moteur propriétaire repris. Authentification ChatGPT officielle conservée.

## État et étapes

| Cycle | Travail | Preuve attendue | État |
|---|---|---|---|
| 1 | En-tête/compositeur terminal ; aide ; catalogue des rôles ; tâches de session ; accès conservé au tableau Codex | Snapshots larges/étroits, dispatch réel des commandes, lancement installé | installé et vérifié ; catalogue encore en lecture seule |
| 2 | Workflows : statut persistant, événements, pause/reprise/arrêt, reprise après modification | Tests déterministes, exécution réelle et reprise sans duplication | contrôles vérifiés avec de vrais processus et inférences ChatGPT ; concurrence et schéma complet ouverts |
| 3 | Panneau TUI des workflows et appel Workflow depuis l'agent | Exécution, contrôle et propagation du sandbox de la session | panneau installé ; outil conversationnel et héritage natif à faire |
| 4 | Sous-agents : restrictions d'outils natives et métadonnées Claude | Tests d'intégration prouvant le refus hors plafond | à faire |
| 5 | Hooks, permissions, skills, MCP, plugins : combler les formats et comportements manquants | Matrice par option/événement avec cas positif et refus | à faire |
| 6 | Raccourcis, contexte, checkpoints, sessions, options CLI | Tests fonctionnels et observation du terminal | à faire |

Chaque cycle : lecture ciblée → changement cohérent → tests → revue adversariale indépendante séquentielle → correction → build/installation quand vérifié. Les étapes suivantes peuvent être précisées à mesure que les interfaces natives sont confirmées. Les comportements non vérifiés restent explicitement ouverts.

## Interfaces et contraintes

- Cycle 1 : `history_cell/session.rs`, `bottom_pane/chat_composer.rs`, `slash_command.rs`, nouveau module `chatwidget/claudex_commands.rs`. Réutiliser les vues de sélection et `OpenAgentPicker` ; préserver le tableau partagé via `/agent-center`.
- Cycle 2 : `scripts/workflows.mjs` et modules adjacents ; checkpoint compatible et borné. Aucun changement aux workflows des dépôts utilisateurs. Un résultat rejoué ne relance pas l'agent.
- Cycle 3 : réutiliser les extensions et événements app-server ; jamais lancer un enfant avec des permissions plus larges que son parent.
- Cycle 4 : `ToolPolicy` native, plafond immuable capturé au lancement/reprise. Une restriction ne doit jamais être remplacée par une simple instruction au modèle.
- Chaque nouveau panneau doit fonctionner sur terminal étroit et rester accessible au clavier ; sauvegarder les changements existants.
- Le runtime JS est réservé aux scripts de confiance : `node:vm` n'est pas un sandbox de sécurité.

## Revue adversariale

Contrôler : dispatch en cours de tâche, serveur embarqué/distant, terminal largeur zéro/étroite/Unicode, injection via nom/description, garde des outils indirects CodeMode/MCP, dérive de permissions à la reprise, crash avec enfants actifs, checkpoint invalide, duplication, propagation des erreurs et limites de contexte. Les tests unitaires ne constituent pas une preuve d'exécution de production de `film.workflow.js`.

## Sources publiques

- [Interaction terminal](https://code.claude.com/docs/en/interactive-mode)
- [Commandes](https://code.claude.com/docs/en/commands)
- [Sous-agents](https://code.claude.com/docs/en/sub-agents)
- [Workflows](https://code.claude.com/docs/en/workflows)
- [Hooks](https://code.claude.com/docs/en/hooks), [permissions](https://code.claude.com/docs/en/permissions)
- [Skills](https://code.claude.com/docs/en/skills), [MCP](https://code.claude.com/docs/en/mcp), [plugins](https://code.claude.com/docs/en/plugins-reference)

Les fonctionnalités dépendant des services Anthropic doivent être distinguées des équivalents réalisables avec Codex. Aucune parité exhaustive n'est acquise à ce stade.

## Journal

- Prévol : dépôt propre à `1223c5a`. Tag `rust-v0.160.0` résolu au commit `a956835d020762cb2b570053af06f643a11c0ecc` ; l'ancien document mentionnait l'objet du tag.
- Cycle TUI/workflows : menus `/help`, `/agents`, `/tasks`, `/agent-center`, `/workflows` implémentés ; catalogue des rôles encore en lecture seule. Statut, contrôle local et reprise du préfixe inchangé ajoutés. Revue adversariale TypeScript puis Rust, sans agents parallèles. Corrections : erreurs de checkpoint propagées, suffixe obsolète invalidé, double `--run-id` refusé, événements locaux conservés hors connexion, lignes d'agents navigables, effets Codex préservés.
- Validation : huit tests du contrôle workflow réussis avec exécuteur déterministe. `workflow-live.test.mjs` a ensuite vérifié de vraies inférences ChatGPT, la pause/reprise, deux résultats rejoués, le refus d'un identifiant doublé et l'arrêt du processus Codex actif sans lancement du suivant. Aucun workflow marketing de production exécuté.
- Dernière suite TUI complète : 5536 réussites, 11 échecs et 2 expirations avant les dernières corrections. Reprises ciblées : 64/66, puis 12/12 après correction des deux échecs restants. La suite complète n'est pas déclarée verte. Build et Clippy ciblés réussis ; binaire installé identique au binaire compilé ; commandes PowerShell/CMD et session ChatGPT vérifiées. Panneaux `/help`, `/agents`, `/tasks`, `/workflows` et détails d'un run effectivement observés ; fermeture propre.
- Outils natifs : crate indépendante `ext/claude-tools` pour Read/Grep/Glob, via l'ExecutorFileSystem de la session. Les 12 tests ciblés passent : exécution directe et CodeMode, refus effectif, priorité des outils client, budget de 8 Kio et restauration Legacy/Paginated ; deux nouveaux tests de budget d'erreur portent le paquet à 11/11 réussites. Suite élargie : 992 exécutés, 990 réussis ; les deux échecs de snapshot et de schéma ont été corrigés puis repris, 2/2 réussis. Rendu TUI examiné aux largeurs 26 et 80. Nouveau binaire compilé et installé, hash identique ; inférences ChatGPT directes et CodeMode et restauration des trois cartes réussies avec catalogue de fixture explicite. Les trois cartes sont aussi observées dans le terminal installé. Annulation persistée, sortie après PostToolUse et événements CLI JSON restent ouverts.
- File workflow : phase capturée à la déclaration et agents en attente visibles immédiatement, commit `238cce0`. Dix tests contrôleur passent, ainsi que le contrat général. Revue adversariale : erreur de sauvegarde interceptable corrigée et reproduite par un test dédié.

### Publication terminale des outils : sources verifiees, installation suivante

Le handler conserve son resultat borne ; le callback de fin transmet `Unchanged / Feedback / Rejected` apres PostToolUse. Un registre de 256 appels maximum par thread publie une seule paire Started/Completed, avec une tache proprietaire independante du callback annule. L'abandon ferme les inscriptions du tour. La fin normale ne les ferme pas : une cellule CodeMode survivante peut encore appeler les outils. Les gardes liberent les slots si le handler ou l'emetteur panique, ou si le runtime est ferme.

Preuves : 24/25 controles integration initialement reussis ; le dernier echec concernait la barriere de fixture inter-tours. Apres remplacement par un fichier executor independant des demandes client, ce test passe (`tests-file-interturn-cycle2.log`). Les 58 controles host/extensions/TUI restants passent ; le dernier test TUI est repris avec succes apres correction du snapshot dans une boucle (`tests-file-late-tui-green.log`). Ces tests utilisent des modeles de fixture ; ce ne sont pas des inferences ChatGPT. Les suites completes ne sont pas declarees vertes.

La TUI conserve 256 cellules terminees pour mettre a jour une carte plutot que d'en ajouter une seconde lors d'une completion tardive. Une eviction peut permettre un doublon ; le scrollback physique deja imprime n'est pas garanti actualise. Une panique du host apres le stockage du resultat et avant le callback de fin reste ouverte. La carte peut alors attendre l'interruption ou l'arret du runtime.

Le contrat CodeMode natif conserve le resultat type original lorsque PostToolUse fournit un feedback ; la sortie directe contient le feedback. La carte affiche le feedback. Aucune barriere generale avant TurnAborted n'est revendiquee. Le nouveau binaire n'est pas encore installe a cette etape.
