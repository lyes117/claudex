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

### Prochain stage : publication terminale des outils

Revue de conception séquentielle effectuée ; implémentation non commencée. Le handler doit conserver son résultat borné sans publier la completion avant PostToolUse. Une disposition interne `Unchanged / Feedback / Rejected` au callback natif de fin évite d'emprunter un `ToolOutput` non `Sync`. Un registre borné par thread, clés `(turn_id, call_id)`, ferme les inscriptions du tour lors de l'abandon. La publication doit être détenue explicitement par l'entrée et attendue par fin et abandon, sans async `Drop` : l'arrêt forcé après 100 ms peut détruire un callback en cours. Tester publication commencée, fin/abandon concurrents, inscriptions tardives, rejet sans sortie originale et reprise des deux historiques.

Le contrat CodeMode natif conserve volontairement le résultat typé original lorsque PostToolUse fournit un feedback, alors que la sortie directe contient le feedback. Afficher ce feedback dans une carte ne démontre pas son remplacement dans JavaScript. Aucune barrière générale avant `TurnAborted` n'est établie pour les appels imbriqués.
